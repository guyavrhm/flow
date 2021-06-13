import time

from pynput.mouse import Controller

from .transfer import SharedDevices
from .vclipboard import ServerClipboard

from src.network.sockets import DifferentEncryption, socket
from src.data.db import Screens, get_attachments
from src.info.computerinfo import get_screeninfo
from src.ui.qtthread import flowThread


class Machine:
    """
    A class used to represent a machine.
    """

    def __init__(self, metrics, attachments, tcp_conn=None, udp_conn=None, address=None, mpos=(100, 100)):
        self.mouse_position = mpos
        self.metrics = metrics
        self.attachments = attachments
        self.tcp_conn = tcp_conn
        self.udp_conn = udp_conn
        self.address = address

    def at_edge(self):
        """
        Returns machine at side near the mouse.
        If mouse is not at edge, returns None.
        """
        if self.mouse_position[0] < 5:
            return self.attachments[Screens.LEFT]
        if self.mouse_position[0] > self.metrics[0] - 5:
            return self.attachments[Screens.RIGHT]
        if self.mouse_position[1] < 5:
            return self.attachments[Screens.TOP]
        if self.mouse_position[1] > self.metrics[1] - 5:
            return self.attachments[Screens.BOTTOM]

    def pass_to(self, machine):
        """
        Changes given machine's mouse position according to current machines mouse position
        """
        if (self.attachments[Screens.LEFT] == machine.address[0]) or (
                self.attachments[Screens.RIGHT] == machine.address[0]):
            ratio = self.metrics[1] / (self.mouse_position[1] + 0.1)
        else:
            ratio = self.metrics[0] / (self.mouse_position[0] + 0.1)

        if self.attachments[Screens.RIGHT] == machine.address[0]:
            machine.mouse_position = (8, int(machine.metrics[1] / ratio))
        if self.attachments[Screens.LEFT] == machine.address[0]:
            machine.mouse_position = (machine.metrics[0] - 8, int(machine.metrics[1] / ratio))
        if self.attachments[Screens.BOTTOM] == machine.address[0]:
            machine.mouse_position = (int(machine.metrics[0] / ratio), 8)
        if self.attachments[Screens.TOP] == machine.address[0]:
            machine.mouse_position = (int(machine.metrics[0] / ratio), machine.metrics[1] - 8)

        if machine.is_server():  # is main
            Controller().position = machine.mouse_position
            Controller().position = machine.mouse_position
            Controller().position = machine.mouse_position

    def close(self):
        for conn in (self.tcp_conn, self.udp_conn):
            if conn is not None:
                conn.close()

    def is_server(self):
        return self.tcp_conn is None


class Server(flowThread):
    """
    Server class used to handle client connection
    and mouse movement.
    """

    NAME = 'main'
    # dictionary of running machines {name: Machine}
    machines = {}

    def __init__(self):
        super().__init__()

        # add the server to attachments
        attachments = get_attachments(self.NAME)
        self.machines[self.NAME] = Machine(
            get_screeninfo(),
            attachments,
            mpos=Controller().position,
            address=(self.NAME,)
        )

        # tcp and udp sockets
        self.udp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.udp_sock.bind(('', 8118))

        self.tcp_sock = socket.socket()
        self.tcp_sock.bind(('', 8118))

        # weather the server is running
        self._running = False

        # accepting clients thread
        self.accept_clients_t = flowThread(target=self.accept_clients)

        # current machine being controlled
        self.current = None

        # hardware to control clients
        self.devices = None
        self.clipboard = ServerClipboard(self)

    def run(self):
        """
        Starts accepting clients.
        """
        self.current = self.machines[self.NAME]

        self.accept_clients_t.start()
        self.clipboard.start()

        self._running = True
        self.runloop()

    def accept_clients(self):
        """
        Accepts and adds new clients to 'machines' attribute.
        """
        self.tcp_sock.listen()
        try:
            while 1:
                try:
                    client, _ = self.tcp_sock.true_accept()
                except DifferentEncryption:
                    continue

                metrics = client.true_recv()
                _, address = self.udp_sock.true_recvfrom(1024)
                client.setblocking(False)

                attachments = get_attachments(address[0])
                self.machines[address[0]] = Machine(
                    metrics,
                    attachments,
                    tcp_conn=client,
                    udp_conn=self.udp_sock,
                    address=address
                )
                self.machine_connected_signal.emit(address[0])
                self.connect_signal.emit()
        except OSError:
            # closed tcp_sock
            return

    def remove_client(self, machine):
        """
        Remove client from current machines and emit disconnect signal to UI.
        """
        self.machine_disconnected_signal.emit(machine.address[0])
        del self.machines[machine.address[0]]

        if len(self.machines) == 1:
            self.disconnect_signal.emit()

        if self.current == machine:
            self.devices.pause()
            self.hide_blocker_signal.emit()
            self.current = self.machines[self.NAME]

    def runloop(self):
        """
        Main server loop. 
        Switches current machine controlled when mouse touches edges of screen.
        """
        while self._running:
            if self.current == self.machines[self.NAME]:
                self.machines[self.NAME].mouse_position = Controller().position

            try:
                other = self.machines[self.current.at_edge()]
            except KeyError:
                other = None

            if other:
                self.current.pass_to(other)

                prev = self.current
                self.current = other

                if self.current.is_server():
                    self.devices.pause()
                    self.hide_blocker_signal.emit()
                    prev.pass_to(self.current)

                if not self.current.is_server():
                    self.show_blocker_signal.emit()
                    prev.pass_to(self.current)
                    self.devices = SharedDevices(self.current)
                    self.devices.share()

            time.sleep(0.01)

    def stop(self):
        # stop mainloop
        self._running = False

        # stop shared devices thread
        try:
            self.devices.stop()
        except AttributeError:
            # devices not initialized
            pass

        self.clipboard.stop()

        # close all connections of machines
        for c in self.machines.values():
            c.close()

        # close accepting clients thread
        self.udp_sock.close()
        self.tcp_sock.close()
        self.accept_clients_t.wait()
