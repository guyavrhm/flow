from PyQt5.QtCore import QThread, pyqtSignal


class flowThread(QThread):
    """
    Qthread with all UI signals to emit.

    :use: Inheritance or flowThread(target=func)
    """

    # set tray icon connected signal
    connect_signal = pyqtSignal()
    # set tray icon disconnected signal
    disconnect_signal = pyqtSignal()
    # set machine connected signal
    machine_connected_signal = pyqtSignal(str)
    # set machine disconnected signal
    machine_disconnected_signal = pyqtSignal(str)
    # show screen blocker signal
    show_blocker_signal = pyqtSignal()
    # hide screen blocker signal
    hide_blocker_signal = pyqtSignal()

    def __init__(self, target=None):
        super().__init__()
        self._target = target

    def run(self):
        if self._target:
            self._target()
