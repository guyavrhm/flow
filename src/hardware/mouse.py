"""
Constants for string to pynput Button conversion +
mouse functions
"""

import time
import threading
from pynput.mouse import Button, Controller as PynputMouseController, Listener as MouseListener


import src.hardware.info as ci
from src.ui.qtthread import flowThread


mbuttons = {
    'left': Button.left,
    'right': Button.right,
    'middle': Button.middle
}

if ci.platform == ci.WINDOWS:
    t1 = Button.x1
    t2 = Button.x2
elif ci.platform == ci.LINUX:
    t1 = Button.button8
    t2 = Button.button9
else:
    t1 = t2 = Button.middle

mbuttons['x2'] = mbuttons['button9'] = t1
mbuttons['x1'] = mbuttons['button8'] = t2


class MouseController(PynputMouseController):
    def press(self, button):
        if isinstance(button, str):
            button = mbuttons.get(button, button)
        super().press(button)

    def release(self, button):
        if isinstance(button, str):
            button = mbuttons.get(button, button)
        super().release(button)


class LockedMouse(MouseListener):
    """
    A class used to collect mouse movement while keeping
    the mouse pointer in the center, using pynput's event-driven MouseListener.
    """

    def __init__(self, on_move):
        self.mouse_controller = MouseController()
        self.metrics = ci.get_screeninfo()
        self.x_center = int(self.metrics[0] / 2)
        self.y_center = int(self.metrics[1] / 2)
        self._callback = on_move

        # Reset cursor to center initially
        self.mouse_controller.position = (self.x_center, self.y_center)
        super().__init__(on_move=self._on_move)

    def _on_move(self, x, y):
        x_movement = x - self.x_center
        y_movement = y - self.y_center
        if x_movement != 0 or y_movement != 0:
            self.mouse_controller.position = (self.x_center, self.y_center)
            self._callback(x_movement, y_movement)

    def wait(self):
        self.join()
