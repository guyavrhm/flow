import queue
from PyQt5.QtCore import QObject, pyqtSignal
from src.ui.qtthread import flowThread

# Thread-safe queue to pass clipboard data from the main GUI thread to the listener background thread
clipboard_queue = queue.Queue()
clipboard_queue_active = False

class ClipboardHelper(QObject):
    """
    QObject instantiated on the main GUI thread.
    Handles thread-safe updates to the QClipboard from background threads.
    """
    set_text_signal = pyqtSignal(str)
    set_files_signal = pyqtSignal(list)

    def __init__(self):
        super().__init__()
        self.set_text_signal.connect(self._set_text)
        self.set_files_signal.connect(self._set_files)

    def _set_text(self, text):
        from PyQt5.QtWidgets import QApplication
        QApplication.clipboard().setText(text)

    def _set_files(self, files):
        from PyQt5.QtWidgets import QApplication
        from PyQt5.QtCore import QMimeData, QUrl
        mime_data = QMimeData()
        urls = [QUrl.fromLocalFile(f) for f in files]
        mime_data.setUrls(urls)
        QApplication.clipboard().setMimeData(mime_data)

# Global helper variable, initialized on the main thread in components.py
clipboard_helper = None

class Clipboard:
    """
    Thread-safe Clipboard interface wrapper.
    """
    @staticmethod
    def set_text(text: str):
        if clipboard_helper:
            clipboard_helper.set_text_signal.emit(text)

    @staticmethod
    def set_files(files: list):
        if clipboard_helper:
            clipboard_helper.set_files_signal.emit(files)


class ClipboardListener(flowThread):
    """
    Thread that processes clipboard change events.
    Blocks indefinitely waiting for events from the main thread (0% CPU).
    """
    def __init__(self, on_change, parent=None, pause=1):
        super().__init__(parent=parent)
        self._callback = on_change

    def run(self):
        global clipboard_queue_active
        clipboard_queue_active = True

        # Clear any stale events in the queue before starting
        while not clipboard_queue.empty():
            try:
                clipboard_queue.get_nowait()
            except queue.Empty:
                break

        recent_value = None
        while True:
            # Block indefinitely (no timeout, 0% CPU). Wakes up instantly on new value.
            tmp = clipboard_queue.get()
            if tmp is None:
                # None is our sentinel shutdown signal
                break
            if tmp != recent_value:
                recent_value = tmp
                self._callback(recent_value)

    def stop(self):
        global clipboard_queue_active
        clipboard_queue_active = False
        # Push sentinel to wake up the thread and exit cleanly
        clipboard_queue.put(None)
        self.wait()


def handle_clipboard_changed():
    """
    Slot triggered on the main thread when QClipboard contents change.
    Reads current clipboard contents and queues it for the background thread.
    """
    if not clipboard_queue_active:
        return

    from PyQt5.QtWidgets import QApplication
    clipboard = QApplication.clipboard()
    mime_data = clipboard.mimeData()
    if mime_data.hasUrls():
        urls = mime_data.urls()
        files = [url.toLocalFile() for url in urls if url.isLocalFile()]
        if files:
            clipboard_queue.put(tuple(files))
            return
    clipboard_queue.put(clipboard.text())
