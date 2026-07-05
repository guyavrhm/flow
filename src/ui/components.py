"""
Global UI components accessible from the main thread.
Initialized lazily to avoid import-time side-effects.
"""

_app = None
_settings = None
_tray_icon = None
_blocker = None
_tray_parent = None

def _init():
    global _app, _settings, _tray_icon, _blocker, _tray_parent
    import sys
    from PyQt5.QtWidgets import QApplication
    from PyQt5 import QtWidgets

    _app = QApplication.instance()
    if not _app:
        _app = QApplication(sys.argv)
        _app.setStyle('Fusion')

    # Import UI classes ONLY after QApplication is created to avoid static QIcon/QPixmap instantiation crashes
    from .qtsettings import SettingsWindow
    from .qttrayicon import TrayIcon
    from .qtblocker import ScreenBlocker
    from src.hardware.clipboard import ClipboardHelper, handle_clipboard_changed
    import src.hardware.clipboard as hc

    # Initialize the clipboard helper and connect the system clipboard signal
    hc.clipboard_helper = ClipboardHelper()
    _app.clipboard().dataChanged.connect(handle_clipboard_changed)

    # Instantiate UI windows
    _settings = SettingsWindow()
    _tray_parent = QtWidgets.QWidget()
    _tray_icon = TrayIcon(_tray_parent)
    _blocker = ScreenBlocker()

def __getattr__(name):
    if name in ('app', 'settings', 'tray_icon', 'blocker'):
        if _app is None:
            _init()
        if name == 'app':
            return _app
        elif name == 'settings':
            return _settings
        elif name == 'tray_icon':
            return _tray_icon
        elif name == 'blocker':
            return _blocker
    raise AttributeError(f"module '{__name__}' has no attribute '{name}'")
