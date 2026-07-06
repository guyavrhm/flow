import time
import logging

from .transfer import ControlledDevices
from .vclipboard import ClientClipboard

from src.data.db import Settings, get_data
from src.network.sockets import DifferentEncryption, socket
from src.info.computerinfo import get_screeninfo
from src.ui.qtthread import flowThread

logger = logging.getLogger(__name__)


class Client(flowThread):
    """
    Client class used to handle server connection.
    """

    def __init__(self):
        super().__init__()
        logger.info("Initializing Client instance")

        # tcp and udp sockets
        self.udp_sock = self.tcp_sock = None

        # connection identifiers
        self.waiting_for_connection = True
        self.connected = False

        # hardware to be controlled
        self.devices = ControlledDevices(self)
        self.clipboard = ClientClipboard(self)

        self._running = True

    def run(self):
        """
        Connectes to a server and starts listening for hardware events.
        """
        logger.info("Starting Client connection flow...")
        self.init_connection()

        if self.connected and self._running:
            logger.info("Successfully connected. Starting clipboard listener and device control loop.")
            self.clipboard.start()
            self.devices.get_controlled()  # blocking
        else:
            logger.info("Client thread execution finished without active connection")

    def init_connection(self):
        """
        Initiates UDP and TCP connection with the server.
        """
        if not self._running:
            return
        logger.info("Initiating server connection attempt...")
        self.disconnect_signal.emit()
        self.connected = False
        self.udp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.udp_sock.bind(('', 0))

        self.waiting_for_connection = True
        while self.waiting_for_connection and self._running:
            try:
                server_ip = get_data(Settings.IP)
                logger.info("Attempting TCP connection to server IP: %s", server_ip)
                self.tcp_sock = socket.socket()
                self.tcp_sock.true_connect((server_ip, 8118))
                self.waiting_for_connection = False
                self.connected = True
                logger.info("TCP connection to server succeeded")
            except (OSError, TimeoutError, ConnectionRefusedError, socket.gaierror, DifferentEncryption) as e:
                logger.warning("TCP connection failed: %s. Retrying in 1s...", e)
                if self.tcp_sock is not None:
                    try:
                        self.tcp_sock.close()
                    except Exception as ex:
                        logger.debug("Failed to close TCP socket during connection retry: %s", ex)
                time.sleep(1)
                continue

        if self.connected and self._running:
            try:
                logger.info("Sending screen metrics and executing UDP handshake...")
                self.tcp_sock.true_send(get_screeninfo())
                self.udp_sock.true_sendto('.', (server_ip, 8118))
                logger.info("UDP handshake packet and screen metrics sent successfully")
            except OSError as e:
                logger.error("Failed to send metrics or UDP handshake to server: %s", e)
                pass
            self.connected = True
            self.connect_signal.emit()

    def reconnect(self):
        """
        Reconnects to a server.
        """
        if not self._running:
            return
        logger.info("Reconnecting client to server...")
        if self.tcp_sock is not None:
            try:
                self.tcp_sock.close()
            except Exception:
                pass
        if self.udp_sock is not None:
            try:
                self.udp_sock.close()
            except Exception:
                pass
        self.init_connection()

    def stop(self):
        logger.info("Stopping Client operations...")
        self._running = False
        self.connected = False
        self.waiting_for_connection = False
        self.clipboard._on = False
        self.devices._on = False

        if self.udp_sock is not None:
            try:
                self.udp_sock.close()
            except Exception as e:
                logger.debug("Failed to close Client UDP socket: %s", e)
        if self.tcp_sock is not None:
            try:
                self.tcp_sock.close()
            except Exception as e:
                logger.debug("Failed to close Client TCP socket: %s", e)
        try:
            logger.info("Stopping client clipboard helper")
            self.clipboard.stop()
        except Exception as e:
            logger.debug("Error while stopping client clipboard: %s", e)
        try:
            logger.info("Stopping client device controller")
            self.devices.stop()
        except Exception as e:
            logger.debug("Error while stopping client device controller: %s", e)


