import sys
import subprocess

import src.info.computerinfo as computerinfo
import src.network.sockets as socket

from src.ui.components import app, settings, tray_icon, blocker
from src.comms.server import Server
from src.comms.client import Client
from src.data.db import Settings, get_data, get_all_data

from .files import WEB_PAGE


class Main:
    """
    flow's main class.
    Responsible for initializing gui components, server/client and
    exception handling.
    """

    def __init__(self):

        settings.onSave(self.save)

        tray_icon.onSettings(self.open_settings)
        tray_icon.onExit(self.exit_flow)
        tray_icon.onHelp(self.open_help)
        tray_icon.show()

        # server or client thread
        self.serverclient = None

        typ = get_data(Settings.PC)
        self.init_serverclient(typ)

        sys.exit(app.exec_())

    @staticmethod
    def open_help():
        """
        Opens the flow web-page.
        """
        if computerinfo.platform == computerinfo.WINDOWS:
            subprocess.Popen(['start', WEB_PAGE])
        elif computerinfo.platform == computerinfo.MACOS:
            subprocess.Popen(['open', WEB_PAGE])
        else:
            subprocess.Popen(['sensible-browser', WEB_PAGE])

    @staticmethod
    def open_settings():
        settings.show()

    def save(self):
        """
        Saves settings.
        Will re-initialize server/client if encryption or
        or server/client specification has changed.
        """

        settings_before = get_all_data()

        settings.update()
        settings.hide()

        settings_after = get_all_data()
        if (
                settings_before[Settings.PC] != settings_after[Settings.PC] or
                settings_before[Settings.ENCRYPTION] != settings_after[Settings.ENCRYPTION] or
                settings_before[Settings.PASS] != settings_after[Settings.PASS]
        ):
            self.stop_serverclient()
            self.init_serverclient(settings_after[Settings.PC])

    def init_serverclient(self, typ):
        """
        Initializes and starts server or client thread
        based on 'typ' argument.
        """
        self.serverclient = None

        if typ == Settings.CLIENT:
            self.serverclient = Client()
        else:
            Server.machines.clear()
            self.serverclient = Server()
            self.serverclient.machine_connected_signal.connect(settings.connect)
            self.serverclient.machine_disconnected_signal.connect(settings.disconnect)
            self.serverclient.show_blocker_signal.connect(blocker.show)
            self.serverclient.hide_blocker_signal.connect(blocker.hide)

        self.serverclient.connect_signal.connect(tray_icon.setConnected)
        self.serverclient.disconnect_signal.connect(tray_icon.setDisconnected)

        if get_data(Settings.ENCRYPTION) == Settings.ENCRYPTION_ON:
            socket.key = socket.set_encryption_key(get_data(Settings.PASS))
        else:
            socket.key = socket.set_encryption_key("")

        self.serverclient.start()

    def stop_serverclient(self):
        """
        Stops the server or client thread from running.
        """
        tray_icon.setIcon(tray_icon.ICON_DISCONNECTED)
        if self.serverclient is not None:
            self.serverclient.stop()
            self.serverclient.wait()
            self.serverclient.deleteLater()

    def exit_flow(self):
        self.stop_serverclient()
        tray_icon.hide()
        app.quit()


if computerinfo.supported():
    Main()
