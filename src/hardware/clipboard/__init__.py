import time

from src.ui.qtthread import flowThread

from ._base import Clipboard


class ClipboardListener(flowThread):
    """
    A thread inherited class used to callback on clipboard change.
    """

    def __init__(self, on_change, pause=1):
        super(ClipboardListener, self).__init__()

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
