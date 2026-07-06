import time

from .transfer import ControlledDevices
from .vclipboard import ClientClipboard

from src.data.db import Settings, get_data
from src.network.sockets import DifferentEncryption, socket
from src.info.computerinfo import get_screeninfo
from src.ui.qtthread import flowThread


class Client(flowThread):
    """
    Client class used to handle server connection.
    """

    def __init__(self):
        super().__init__()

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
        self.init_connection()

        if self.connected and self._running:
            self.clipboard.start()
            self.devices.get_controlled()  # blocking

    def init_connection(self):
        """
        Initiates UDP and TCP connection with the server.
        """
        if not self._running:
            return
        self.disconnect_signal.emit()
        self.connected = False
        self.udp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.udp_sock.bind(('', 0))

        self.waiting_for_connection = True
        while self.waiting_for_connection and self._running:
            try:
                server_ip = get_data(Settings.IP)
                self.tcp_sock = socket.socket()
                self.tcp_sock.true_connect((server_ip, 8118))
                self.waiting_for_connection = False
                self.connected = True
            except (OSError, TimeoutError, ConnectionRefusedError, socket.gaierror, DifferentEncryption):
                # OSError: no route to host -> host not up
                # TimeError, ConnectionRefused error: host not connected to flow
                # socket.gaierror: invalid ip address
                if self.tcp_sock is not None:
                    try:
                        self.tcp_sock.close()
                    except Exception:
                        pass
                time.sleep(1)
                continue

        if self.connected and self._running:
            try:
                self.tcp_sock.true_send(get_screeninfo())
                self.udp_sock.true_sendto('.', (server_ip, 8118))
            except OSError:
                # when socket closes before initialized
                pass
            self.connected = True
            self.connect_signal.emit()

    def reconnect(self):
        """
        Reconnects to a server.
        """
        if not self._running:
            return
        self.tcp_sock.close()
        self.udp_sock.close()
        self.init_connection()

    def stop(self):
        self._running = False
        self.connected = False
        self.waiting_for_connection = False
        self.clipboard._on = False
        self.devices._on = False

        if self.udp_sock is not None:
            try:
                self.udp_sock.close()
            except Exception:
                pass
        if self.tcp_sock is not None:
            try:
                self.tcp_sock.close()
            except Exception:
                pass
        try:
            self.clipboard.stop()
        except Exception:
            pass
        try:
            self.devices.stop()
        except Exception:
            pass

