from PyQt5.QtCore import QThread, pyqtSignal
import logging

logger = logging.getLogger(__name__)


def wrap_run(run_func, class_name):
    def wrapped(self, *args, **kwargs):
        try:
            return run_func(self, *args, **kwargs)
        except Exception as e:
            logger.exception("Uncaught exception in background thread %s", self.objectName() or class_name)
    return wrapped


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

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if 'run' in cls.__dict__:
            cls.run = wrap_run(cls.run, cls.__name__)

    def __init__(self, target=None, parent=None):
        super().__init__(parent)
        self._target = target

    def run(self):
        try:
            if self._target:
                self._target()
        except Exception as e:
            logger.exception("Uncaught exception in background thread %s", self.objectName() or self.__class__.__name__)

