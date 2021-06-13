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

    def run(self):
        """
        Connectes to a server and starts listening for hardware events.
        """
        self.init_connection()

        if self.connected:
            self.clipboard.start()
            self.devices.get_controlled()  # blocking

    def init_connection(self):
        """
        Initiates UDP and TCP connection with the server.
        """
        self.disconnect_signal.emit()
        self.udp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.udp_sock.bind(('', 8118))

        self.waiting_for_connection = True
        while self.waiting_for_connection:
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
                time.sleep(1)
                continue

        if self.connected:
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
        self.tcp_sock.close()
        self.udp_sock.close()
        self.init_connection()

    def stop(self):
        self.connected = False
        self.waiting_for_connection = False
        self.udp_sock.close()
        self.tcp_sock.close()
        try:
            self.clipboard.stop()
            self.devices.stop()
        except AttributeError:
            # not initialized
            pass
