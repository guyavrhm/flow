from PyQt5 import QtGui, QtWidgets

from src.files import FLOWX_PNG, FLOWV_PNG

import src.ui.msgs as msgs


class TrayIcon(QtWidgets.QSystemTrayIcon):
    """
    flow's tray icon
    """

    # Connected icon: green check
    ICON_CONNECTED = QtGui.QIcon(FLOWV_PNG)
    # Disconnected icon: red cross
    ICON_DISCONNECTED = QtGui.QIcon(FLOWX_PNG)

    def __init__(self, parent):
        QtWidgets.QSystemTrayIcon.__init__(self, self.ICON_DISCONNECTED, parent)

        # set hover text
        self.setToolTip(msgs.NAME)

        menu = QtWidgets.QMenu(parent)

        self.open_set = menu.addAction(msgs.TRAY_SETTNGS)
        self.open_help = menu.addAction(msgs.TRAY_HELP)
        self.exit_ = menu.addAction(msgs.TRAY_EXIT)

        menu.addSeparator()
        self.setContextMenu(menu)

    def setConnected(self):
        """
        Sets icon to connected icon.
        """
        self.setIcon(self.ICON_CONNECTED)

    def setDisconnected(self):
        """
        Sets icon to disconnected icon.
        """
        self.setIcon(self.ICON_DISCONNECTED)

    def onHelp(self, func):
        """
        Sets help click event to given function.
        """
        self.open_help.triggered.connect(func)

    def onSettings(self, func):
        """
        Sets settings click event to given function.
        """
        self.open_set.triggered.connect(func)

    def onExit(self, func):
        """
        Sets exit click event to given function.
        """
        self.exit_.triggered.connect(func)
