"""
Constants for string to pynput Button conversion +
mouse functions
"""

import threading
from pynput.mouse import Button, Controller as MouseController, Listener as MouseListener

import src.info.computerinfo as ci
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


class LockedMouse(flowThread):
    """
    A thread inherited class used to collect mouse movement while keeping
    the mouse pointer in the center.
    """

    def __init__(self, on_move):
        super(LockedMouse, self).__init__()

        self.mouse = MouseController()
        self.metrics = ci.get_screeninfo()
        self.x_center = int(self.metrics[0] / 2)
        self.y_center = int(self.metrics[1] / 2)

        self._callback = on_move
        self._stopping = False

    def run(self):
        """
        Callbacks mouse location relative to center of screen.
        """
        start_x = self.x_center
        start_y = self.y_center
        self.mouse.position = (start_x, start_y)

        while not self._stopping:
            if self.mouse.position != (start_x, start_y):
                x_movement = self.mouse.position[0] - start_x
                y_movement = self.mouse.position[1] - start_y
                if not self._stopping:
                    self.mouse.position = (start_x, start_y)
                    self._callback(x_movement, y_movement)

    def stop(self):
        self._stopping = True
