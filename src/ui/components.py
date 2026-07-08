"""
Global UI components accessible from the main thread.
Initialized lazily to avoid import-time side-effects.
"""

_app = None
_settings = None
_tray_icon = None
_tray_parent = None

def _init():
    global _app, _settings, _tray_icon, _tray_parent
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

    # Instantiate UI windows
    _settings = SettingsWindow()
    _tray_parent = QtWidgets.QWidget()
    _tray_icon = TrayIcon(_tray_parent)

def __getattr__(name):
    if name in ('app', 'settings', 'tray_icon'):
        if _app is None:
            _init()
        if name == 'app':
            return _app
        elif name == 'settings':
            return _settings
        elif name == 'tray_icon':
            return _tray_icon
    raise AttributeError(f"module '{__name__}' has no attribute '{name}'")

