import sys
import subprocess

import src.logger
src.logger.setup_logging()

import logging
logger = logging.getLogger(__name__)


def handle_exception(exc_type, exc_value, exc_traceback):
    """
    Global exception hook to capture uncaught exceptions in the main thread.
    Logs the error and cleanly exits the application.
    """
    if issubclass(exc_type, KeyboardInterrupt):
        sys.__excepthook__(exc_type, exc_value, exc_traceback)
        return

    logger.critical("Uncaught exception in main thread", exc_info=(exc_type, exc_value, exc_traceback))
    sys.exit(1)


sys.excepthook = handle_exception

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
        logger.info("Initializing flow application...")

        settings.onSave(self.save)

        tray_icon.onSettings(self.open_settings)
        tray_icon.onExit(self.exit_flow)
        tray_icon.onHelp(self.open_help)
        tray_icon.show()

        # server or client thread
        self.serverclient = None

        typ = get_data(Settings.PC)
        logger.info("Application starting in %s mode", "SERVER" if typ == Settings.SERVER else "CLIENT")
        self.init_serverclient(typ)

        sys.exit(app.exec_())

    @staticmethod
    def open_help():
        """
        Opens the flow web-page.
        """
        logger.info("Opening help webpage: %s", WEB_PAGE)
        if computerinfo.platform == computerinfo.WINDOWS:
            subprocess.Popen(f'start {WEB_PAGE}', shell=True)
        elif computerinfo.platform == computerinfo.MACOS:
            subprocess.Popen(f'open {WEB_PAGE}', shell=True)
        else:
            subprocess.Popen(f'sensible-browser {WEB_PAGE}', shell=True)

    @staticmethod
    def open_settings():
        logger.info("Opening settings UI")
        settings.show()

    def save(self):
        """
        Saves settings.
        Will re-initialize server/client if encryption or
        or server/client specification has changed.
        """
        logger.info("Saving settings change from UI...")
        settings_before = get_all_data()

        settings.update()
        settings.hide()

        settings_after = get_all_data()
        if (
                settings_before[Settings.PC] != settings_after[Settings.PC] or
                settings_before[Settings.ENCRYPTION] != settings_after[Settings.ENCRYPTION] or
                settings_before[Settings.PASS] != settings_after[Settings.PASS] or
                settings_before[Settings.IP] != settings_after[Settings.IP]
        ):
            logger.info("Settings changed (type, encryption, password, or IP). Re-initializing server/client.")
            self.stop_serverclient()
            self.init_serverclient(settings_after[Settings.PC])
        else:
            logger.info("Settings updated, but changes did not require connection re-initialization.")

    def init_serverclient(self, typ):
        """
        Initializes and starts server or client thread
        based on 'typ' argument.
        """
        logger.info("Initializing %s helper", "SERVER" if typ == Settings.SERVER else "CLIENT")
        self.serverclient = None

        if typ == Settings.CLIENT:
            self.serverclient = Client()
        else:
            with Server.machines_lock:
                Server.machines.clear()
            self.serverclient = Server()
            self.serverclient.machine_connected_signal.connect(settings.connect)
            self.serverclient.machine_disconnected_signal.connect(settings.disconnect)
            self.serverclient.show_blocker_signal.connect(blocker.show)
            self.serverclient.hide_blocker_signal.connect(blocker.hide)

        self.serverclient.connect_signal.connect(tray_icon.setConnected)
        self.serverclient.disconnect_signal.connect(tray_icon.setDisconnected)

        if get_data(Settings.ENCRYPTION) == Settings.ENCRYPTION_ON:
            logger.info("Encryption is enabled; setting key from password")
            socket.key = socket.set_encryption_key(get_data(Settings.PASS))
        else:
            logger.info("Encryption is disabled; setting empty encryption key")
            socket.key = socket.set_encryption_key("")

        logger.info("Starting connection helper thread")
        self.serverclient.start()

    def stop_serverclient(self):
        """
        Stops the server or client thread from running.
        """
        logger.info("Stopping connection helper thread...")
        tray_icon.setIcon(tray_icon.ICON_DISCONNECTED)
        if self.serverclient is not None:
            self.serverclient.stop()
            self.serverclient.finished.connect(self.serverclient.deleteLater)
            self.serverclient = None
            logger.info("Connection helper thread stop signal sent")

    def exit_flow(self):
        logger.info("Exiting flow application gracefully")
        self.stop_serverclient()
        tray_icon.hide()
        app.quit()


if computerinfo.supported():
    Main()
else:
    print("Error: Platform not supported", file=sys.stderr)

