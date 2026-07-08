import time
import threading
import logging

from src.hardware.mouse import MouseController

from .transfer import SharedDevices
from .vclipboard import ServerClipboard

from src.network.sockets import DifferentEncryption, socket
from src.data.db import Screens, get_attachments
from src.hardware.info import get_screeninfo
from src.ui.qtthread import flowThread

logger = logging.getLogger(__name__)


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
            MouseController().position = machine.mouse_position


    def close(self):
        logger.info("Closing connection and resources for machine: %s", self.address)
        if self.tcp_conn is not None:
            try:
                self.tcp_conn.close()
            except Exception as e:
                logger.debug("Failed to close TCP connection for machine %s: %s", self.address, e)
        if self.clipboard_thread is not None:
            try:
                from PyQt5.QtCore import QThread
                if QThread.currentThread() != self.clipboard_thread:
                    self.clipboard_thread.wait()
                self.clipboard_thread.deleteLater()
            except Exception as e:
                logger.debug("Failed to clean up clipboard thread for machine %s: %s", self.address, e)
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
        logger.info("Initializing Server instance")

        # add the server to attachments
        attachments = get_attachments(self.NAME)
        with self.machines_lock:
            self.machines[self.NAME] = Machine(
                get_screeninfo(),
                attachments,
                mpos=MouseController().position,
                address=(self.NAME,)
            )


        # tcp and udp sockets
        logger.info("Binding server sockets on port 8118")
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
        logger.info("Starting Server thread execution")
        with self.machines_lock:
            self.current = self.machines[self.NAME]

        self.accept_clients_t.start()
        self.clipboard.start()

        self._running = True
        try:
            self.runloop()
        finally:
            logger.info("Server thread cleaning up resources...")
            self.cleanup()

    def accept_clients(self):
        """
        Accepts and adds new clients to 'machines' attribute.
        """
        logger.info("Server listening for TCP connections on port 8118...")
        self.tcp_sock.listen()
        try:
            while 1:
                try:
                    client, client_addr = self.tcp_sock.true_accept()
                    logger.info("Accepted TCP connection from %s", client_addr)
                except DifferentEncryption:
                    logger.warning("Rejected client connection: mismatched encryption configuration")
                    continue
                except Exception as e:
                    if not self._running:
                        break
                    logger.debug("Exception in TCP true_accept: %s", e)
                    continue

                try:
                    logger.info("Waiting for client screen info...")
                    metrics = client.true_recv()
                    
                    client_ip = client.getpeername()[0]
                    logger.info("Received client metrics: %s. Handshaking UDP with IP: %s", metrics, client_ip)
                    
                    self.udp_sock.settimeout(3.0)
                    address = None
                    start_time = time.time()
                    while time.time() - start_time < 3.0 and self._running:
                        try:
                            _, addr = self.udp_sock.true_recvfrom(1024)
                            if addr[0] == client_ip:
                                address = addr
                                logger.info("UDP connection handshake succeeded with client address: %s", address)
                                break
                        except socket.timeout:
                            logger.warning("UDP handshake timed out for client IP: %s", client_ip)
                            break
                        except Exception as e:
                            if not self._running:
                                break
                            logger.debug("Exception during UDP handshake check: %s", e)
                            continue
                    self.udp_sock.settimeout(None)

                    if address is None:
                        raise ConnectionError("UDP handshake timed out or failed")

                    attachments = get_attachments(address[0])
                    old_machine = None
                    with self.machines_lock:
                        if address[0] in self.machines:
                            logger.info("Re-registering existing machine: %s", address[0])
                            old_machine = self.machines.pop(address[0])
                        machine = Machine(
                            metrics,
                            attachments,
                            tcp_conn=client,
                            udp_conn=self.udp_sock,
                            address=address
                        )
                        self.machines[address[0]] = machine
                        logger.info("Machine %s successfully registered in active machine list", address[0])

                    if old_machine is not None:
                        try:
                            old_machine.close()
                        except Exception as e:
                            logger.debug("Error closing old machine resource: %s", e)

                    # Start clipboard receiver thread for this client
                    logger.info("Initializing clipboard receiver for client machine: %s", address[0])
                    self.clipboard.start_client_receiver(machine)

                    self.machine_connected_signal.emit(address[0])
                    self.connect_signal.emit()
                except Exception as e:
                    logger.error("Failed to fully establish client connection: %s", e)
                    try:
                        client.close()
                    except Exception:
                        pass
                    continue
        except OSError:
            # closed tcp_sock
            logger.info("TCP socket closed; terminating accept_clients loop")
            return

    def remove_client(self, machine):
        """
        Remove client from current machines and emit disconnect signal to UI.
        """
        client_name = machine.address[0] if machine.address else "Unknown"
        logger.info("Removing client: %s", client_name)
        with self.machines_lock:
            is_active = self.machines.get(client_name) is machine
            if is_active:
                del self.machines[client_name]
            num_machines = len(self.machines)

            if not is_active:
                logger.debug("Client %s was not actively registered under this instance", client_name)
                return

            try:
                machine.close()
            except Exception as e:
                logger.debug("Exception while closing machine resources: %s", e)

            self.machine_disconnected_signal.emit(client_name)

            if num_machines == 1:
                logger.info("All clients disconnected. Emitting disconnect signal to UI.")
                self.disconnect_signal.emit()

            if self.current == machine:
                logger.info("Removed client %s was the active machine; reverting control to server", client_name)
                if self.devices is not None:
                    self.devices.pause()
                    self.devices = None
                self.hide_blocker_signal.emit()
                self.current = self.machines[self.NAME]

    def runloop(self):
        """
        Main server loop. 
        Switches current machine controlled when mouse touches edges of screen.
        """
        logger.info("Entering main server edge tracking loop")
        while self._running:
            with self.machines_lock:
                is_main = (self.current == self.machines[self.NAME])
                if is_main:
                    self.machines[self.NAME].mouse_position = MouseController().position

                try:
                    edge = self.current.at_edge()
                    other = self.machines[edge] if edge in self.machines else None
                except KeyError:
                    other = None

                if other:
                    from_addr = self.current.address[0] if self.current.address else "Unknown"
                    to_addr = other.address[0] if other.address else "Unknown"
                    logger.info("Mouse reached edge. Transferring control from %s to %s", from_addr, to_addr)
                    self.current.pass_to(other)

                    prev = self.current
                    self.current = other
                    
                    if self.devices is not None:
                        self.devices.pause()

                    if self.current.is_server():
                        self.devices = None
                        self.hide_blocker_signal.emit()
                        prev.pass_to(self.current)
                    else:
                        self.show_blocker_signal.emit()
                        prev.pass_to(self.current)
                        self.devices = SharedDevices(self.current)
                        self.devices.share()

            time.sleep(0.01)

    def stop(self):
        logger.info("Stopping Server...")
        self._running = False

        # Close sockets immediately on UI thread to release ports and unblock sockets
        try:
            self.udp_sock.close()
        except Exception as e:
            logger.debug("Error closing UDP socket: %s", e)
        try:
            self.tcp_sock.close()
        except Exception as e:
            logger.debug("Error closing TCP socket: %s", e)

    def cleanup(self):
        # Stop shared devices thread
        with self.machines_lock:
            if self.devices is not None:
                try:
                    logger.info("Stopping shared devices")
                    self.devices.stop()
                except Exception as e:
                    logger.debug("Error while stopping shared devices: %s", e)
                self.devices = None

        self.clipboard.stop()

        # Close all connections of machines
        with self.machines_lock:
            machines_copy = list(self.machines.values())
        logger.info("Closing connections for all %d registered machines", len(machines_copy))
        for c in machines_copy:
            c.close()

        # Clean up accepting clients thread
        try:
            logger.info("Waiting for client acceptor thread to finish...")
            self.accept_clients_t.wait()
            self.accept_clients_t.deleteLater()
            logger.info("Client acceptor thread stopped")
        except Exception as e:
            logger.debug("Error cleaning up client acceptor thread: %s", e)


