"""
Global UI components accessible from the main thread.
"""
import sys

from PyQt5.QtWidgets import QApplication
from PyQt5 import QtWidgets

# init QApplication app
app = QApplication(sys.argv)
app.setStyle('Fusion')

from .qtsettings import SettingsWindow
from .qttrayicon import TrayIcon
from .qtblocker import ScreenBlocker

# settings window
settings = SettingsWindow()

# tray icon
_w = QtWidgets.QWidget()
tray_icon = TrayIcon(_w)

# screen blocker
blocker = ScreenBlocker()
