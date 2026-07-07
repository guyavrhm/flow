import time

from src.ui.qtthread import flowThread

import sys

if sys.platform == 'darwin':
    from ._darwin import MacOSClipboard as Clipboard
elif sys.platform == 'win32':
    from ._win32 import WindowsClipboard as Clipboard
else:
    from ._xorg import LinuxClipboard as Clipboard


class ClipboardListener(flowThread):
    """
    A thread inherited class used to callback on clipboard change.
    """

    def __init__(self, on_change, pause=1, parent=None):
        super(ClipboardListener, self).__init__(parent=parent)

        # function to callback
        self._callback = on_change

        self._pause = pause
        self._stopping = False

    def run(self):
        """
        Callbacks on clipboard change.
        """
        recent_value = ""
        while not self._stopping:
            tmp = Clipboard.data()
            if tmp != recent_value:
                recent_value = tmp
                self._callback(recent_value)
            time.sleep(self._pause)

    def stop(self):
        self._stopping = True
