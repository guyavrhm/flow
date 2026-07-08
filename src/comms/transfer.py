import time
import logging

from src.hardware.keyboard import KeyboardController, KeyboardListener, key_from_str
from src.hardware.mouse import MouseController, MouseListener

logger = logging.getLogger(__name__)


class SharedDevices:
    """
    A class used to send mouse and keyboard data
    to the given client.

    UDP communication
    """

    def __init__(self, machine):
        # current machine being controlled. Send events to this machine
        self.machine = machine
        # current machine's udp socket
        self.socket = machine.udp_conn
        # devices which callback the events
        self.mouse = self.keyboard = None
        self._stopping = False

    def __on_move(self, x, y):
        """
        On move event. Sends 'mov <mouse_position>' to client
        """
        if self._stopping:
            return
        new_x = max(0, min(self.machine.mouse_position[0] + x, self.machine.metrics[0]))
        new_y = max(0, min(self.machine.mouse_position[1] + y, self.machine.metrics[1]))
        self.machine.mouse_position = (new_x, new_y)
        self.socket.true_sendto(
            "mov " + str(new_x) + " " + str(new_y),
            self.machine.address)

    def __on_mouse_click(self, x, y, button, pressed):
        """
        On mouse click event. Sends 'prsm <button>' to client
        """
        if self._stopping:
            return
        self.socket.true_sendto(("prsm", pressed, str(button)[7:]), self.machine.address, special=True)

    def __on_mouse_scroll(self, x, y, dx, dy):
        """
        On scroll event. Sends 'scrl <dx dy>' to client
        """
        if self._stopping:
            return
        self.socket.true_sendto("scrl " + str(dx) + " " + str(dy), self.machine.address)

    def __on_keyboard_press(self, key):
        """
        On key press event. Sends 'prsk True <key>' to client
        """
        if self._stopping:
            return
        self.socket.true_sendto(("prsk", True, str(key)), self.machine.address, special=True)

    def __on_keyboard_release(self, key):
        """
        On key release event. Sends 'prsk False <key>' to client
        """
        if self._stopping:
            return
        self.socket.true_sendto(("prsk", False, str(key)), self.machine.address, special=True)

    def share(self):
        """
        Starts mouse and keyboard event capture
        """
        logger.info("Starting input device capture (mouse and keyboard sharing)")
        self.mouse = MouseListener(
            on_move=self.__on_move,
            on_click=self.__on_mouse_click,
            on_scroll=self.__on_mouse_scroll,
            suppress=True
        )
        self.keyboard = KeyboardListener(
            on_press=self.__on_keyboard_press,
            on_release=self.__on_keyboard_release,
            suppress=True
        )
        self.mouse.start()
        self.keyboard.start()

    def pause(self):
        logger.info("Pausing/stopping input device capture")
        if self.mouse is not None:
            self._stopping = True
            for device in (self.mouse, self.keyboard):
                device.stop()

            # Join them synchronously with a small timeout to prevent blocking the
            # server edge tracking loop while ensuring thread resources are cleaned up.
            try:
                self.mouse.join(timeout=0.2)
            except Exception as e:
                logger.debug("Error joining mouse: %s", e)
            try:
                self.keyboard.join(timeout=0.2)
            except Exception as e:
                logger.debug("Error joining keyboard: %s", e)

    def stop(self):
        logger.info("Stopping shared devices helper and sending stop event to client")
        self.pause()
        try:
            self.socket.true_sendto("stp X X", self.machine.address)
        except Exception as e:
            logger.debug("Failed to send stop command to client: %s", e)


class ControlledDevices:
    """
    A class used to receive mouse and keyboard data
    and physically apply them.

    UDP communication
    """

    def __init__(self, client):
        # current controlled client
        self.client = client

        # devices controllers
        self.keyboard = KeyboardController()
        self.mouse = MouseController()

        self._on = True

    def get_controlled(self):
        """
        Implements events received from server
        """
        logger.info("Starting hardware control loop")
        while self._on:
            try:
                data = self.client.udp_sock.true_recvfrom(1024)[0]
                if type(data) == str:
                    data = data.split(" ")

                cmd_type = data[0]
                action = (data[1], data[2])

                if cmd_type != "mov":
                    logger.debug("Processing command: %s with action: %s", cmd_type, action)

                if cmd_type == "mov":
                    x_pos, y_pos = action
                    self.mouse.position = (int(float(x_pos)), int(float(y_pos)))

                elif cmd_type == "prsk":
                    # data = True/False, key:
                    pressed, key = action
                    try:
                        if pressed:
                            self.keyboard.press(key_from_str(key))
                        else:
                            self.keyboard.release(key_from_str(key))
                    except KeyError:
                        logger.warning("Unmapped key requested by server: %s", key)
                        pass

                elif cmd_type == "prsm":
                    # data = True/False, button:
                    pressed, str_button = action
                    if pressed:
                        self.mouse.press(str_button)
                    else:
                        self.mouse.release(str_button)

                elif cmd_type == "scrl":
                    # data = dx, dy
                    dx, dy = action
                    self.mouse.scroll(int(dx), int(dy))

                elif cmd_type == "stp":
                    # data = 'X', 'X'
                    logger.info("Received stop notification from server")
                    pass
            except Exception as e:  # when the udp socket is closed/reconnecting or packet decryption/unpickling fails
                if not self._on:
                    break
                logger.debug("Exception in get_controlled loop: %s", e)
                time.sleep(1)
                continue

    def stop(self):
        logger.info("Stopping hardware control loop")
        self._on = False

