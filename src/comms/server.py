import time
import threading

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
        self.clipboard_thread = None

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
        if self.tcp_conn is not None:
            try:
                self.tcp_conn.close()
            except Exception:
                pass
        if self.clipboard_thread is not None:
            try:
                from PyQt5.QtCore import QThread
                if QThread.currentThread() != self.clipboard_thread:
                    self.clipboard_thread.wait()
                self.clipboard_thread.deleteLater()
            except Exception:
                pass
            self.clipboard_thread = None

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
    machines_lock = threading.Lock()

    def __init__(self):
        super().__init__()

        # add the server to attachments
        attachments = get_attachments(self.NAME)
        with self.machines_lock:
            self.machines[self.NAME] = Machine(
                get_screeninfo(),
                attachments,
                mpos=Controller().position,
                address=(self.NAME,)
            )

        # tcp and udp sockets
        self.udp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.udp_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.udp_sock.bind(('', 8118))

        self.tcp_sock = socket.socket()
        self.tcp_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.tcp_sock.bind(('', 8118))

        # weather the server is running
        self._running = False

        # accepting clients thread
        self.accept_clients_t = flowThread(target=self.accept_clients, parent=self)

        # current machine being controlled
        self.current = None

        # hardware to control clients
        self.devices = None
        self.clipboard = ServerClipboard(self)

    def run(self):
        """
        Starts accepting clients.
        """
        with self.machines_lock:
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

                try:
                    metrics = client.true_recv()
                    
                    client_ip = client.getpeername()[0]
                    self.udp_sock.settimeout(3.0)
                    address = None
                    start_time = time.time()
                    while time.time() - start_time < 3.0 and self._running:
                        try:
                            _, addr = self.udp_sock.true_recvfrom(1024)
                            if addr[0] == client_ip:
                                address = addr
                                break
                        except socket.timeout:
                            break
                        except Exception:
                            if not self._running:
                                break
                            continue
                    self.udp_sock.settimeout(None)

                    if address is None:
                        raise ConnectionError("UDP handshake timed out or failed")

                    attachments = get_attachments(address[0])
                    old_machine = None
                    with self.machines_lock:
                        if address[0] in self.machines:
                            old_machine = self.machines.pop(address[0])
                        machine = Machine(
                            metrics,
                            attachments,
                            tcp_conn=client,
                            udp_conn=self.udp_sock,
                            address=address
                        )
                        self.machines[address[0]] = machine

                    if old_machine is not None:
                        try:
                            old_machine.close()
                        except Exception:
                            pass

                    # Start clipboard receiver thread for this client
                    self.clipboard.start_client_receiver(machine)

                    self.machine_connected_signal.emit(address[0])
                    self.connect_signal.emit()
                except Exception:
                    try:
                        client.close()
                    except Exception:
                        pass
                    continue
        except OSError:
            # closed tcp_sock
            return

    def remove_client(self, machine):
        """
        Remove client from current machines and emit disconnect signal to UI.
        """
        with self.machines_lock:
            is_active = self.machines.get(machine.address[0]) is machine
            if is_active:
                del self.machines[machine.address[0]]
            num_machines = len(self.machines)

        if not is_active:
            return

        try:
            machine.close()
        except Exception:
            pass

        self.machine_disconnected_signal.emit(machine.address[0])

        if num_machines == 1:
            self.disconnect_signal.emit()

        if self.current == machine:
            if self.devices is not None:
                self.devices.pause()
                self.devices = None
            self.hide_blocker_signal.emit()
            with self.machines_lock:
                self.current = self.machines[self.NAME]

    def runloop(self):
        """
        Main server loop. 
        Switches current machine controlled when mouse touches edges of screen.
        """
        while self._running:
            with self.machines_lock:
                is_main = (self.current == self.machines[self.NAME])
                if is_main:
                    self.machines[self.NAME].mouse_position = Controller().position

            try:
                with self.machines_lock:
                    edge = self.current.at_edge()
                    other = self.machines[edge] if edge in self.machines else None
            except KeyError:
                other = None

            if other:
                self.current.pass_to(other)

                prev = self.current
                self.current = other
                
                if self.devices != None:
                    self.devices.pause()

                if self.current.is_server():
                    self.devices = None
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
        if self.devices is not None:
            try:
                self.devices.stop()
            except Exception:
                pass

        self.clipboard.stop()

        # close all connections of machines
        with self.machines_lock:
            machines_copy = list(self.machines.values())
        for c in machines_copy:
            c.close()

        # close accepting clients thread
        try:
            self.udp_sock.close()
        except Exception:
            pass
        try:
            self.tcp_sock.close()
        except Exception:
            pass
        try:
            self.accept_clients_t.wait()
            self.accept_clients_t.deleteLater()
        except Exception:
            pass

