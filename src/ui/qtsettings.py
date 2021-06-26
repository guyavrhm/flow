from PyQt5 import QtCore, QtGui, QtWidgets
from PyQt5.QtWidgets import QListWidget, QMainWindow

from src.ui.qtscreens import GraphicView
from src.files import FLOW_PNG
from src.data.db import Settings, get_data, remove_screen, set_data
from src.info.computerinfo import get_ip
from src.comms.server import Server

import src.ui.msgs as msgs


class SettingsWindow(QMainWindow):
    """
    flow's settings menu.
    """

    def __init__(self, *args, **kwargs):
        super(SettingsWindow, self).__init__(*args, **kwargs)

        self.setWindowTitle(msgs.NAME)
        self.setFixedSize(1013, 611)
        self.setWindowIcon(QtGui.QIcon(FLOW_PNG))

        font = QtGui.QFont()
        font.setPixelSize(13)

        self.frame = QtWidgets.QFrame(self)
        self.frame.setGeometry(QtCore.QRect(30, 40, 341, 141))
        self.frame.setFrameShape(QtWidgets.QFrame.Box)
        self.frame.setFrameShadow(QtWidgets.QFrame.Sunken)
        self.frame.setLineWidth(1)
        self.frame_2 = QtWidgets.QFrame(self)
        self.frame_2.setGeometry(QtCore.QRect(30, 190, 341, 141))
        self.frame_2.setFrameShape(QtWidgets.QFrame.Box)
        self.frame_2.setFrameShadow(QtWidgets.QFrame.Sunken)
        self.frame_2.setLineWidth(1)
        self.frame_3 = QtWidgets.QFrame(self)
        self.frame_3.setGeometry(QtCore.QRect(30, 340, 341, 141))
        self.frame_3.setFrameShape(QtWidgets.QFrame.Box)
        self.frame_3.setFrameShadow(QtWidgets.QFrame.Sunken)

        self.radioButton_server = QtWidgets.QRadioButton(self)
        self.radioButton_server.setGeometry(QtCore.QRect(60, 60, 20, 17))
        self.radioButton_server.setChecked(True)
        self.radioButton_client = QtWidgets.QRadioButton(self)
        self.radioButton_client.setGeometry(QtCore.QRect(60, 210, 20, 17))

        self.label_radioButton_server = QtWidgets.QLabel(self)
        self.label_radioButton_server.setGeometry(QtCore.QRect(80, 60, 291, 17))
        self.label_radioButton_server.setFont(font)
        self.label_radioButton_server.setText(msgs.SERVER_INF)
        self.label_radioButton_client = QtWidgets.QLabel(self)
        self.label_radioButton_client.setGeometry(QtCore.QRect(80, 210, 82, 17))
        self.label_radioButton_client.setFont(font)
        self.label_radioButton_client.setText(msgs.CLIENT_INF)
        self.label_local = QtWidgets.QLabel(self)
        self.label_local.setGeometry(QtCore.QRect(80, 100, 271, 16))
        self.label_local.setFont(font)
        self.label_local.setText(f"{msgs.IP_INF} {get_ip()}")
        self.label_enterip = QtWidgets.QLabel(self)
        self.label_enterip.setGeometry(QtCore.QRect(80, 250, 111, 16))
        self.label_enterip.setFont(font)
        self.label_enterip.setText(msgs.ENTER_IP)
        self.label_password = QtWidgets.QLabel(self)
        self.label_password.setGeometry(QtCore.QRect(80, 400, 61, 16))
        self.label_password.setFont(font)
        self.label_password.setText(msgs.PASS_INF)
        self.label_encrypt = QtWidgets.QLabel(self)
        self.label_encrypt.setGeometry(QtCore.QRect(80, 360, 71, 16))
        self.label_encrypt.setFont(font)
        self.label_encrypt.setText(msgs.ENC_INF)

        self.pushButton_start = QtWidgets.QPushButton(self)
        self.pushButton_start.setGeometry(QtCore.QRect(140, 510, 111, 41))
        self.pushButton_start.setFont(font)
        self.pushButton_start.setText(msgs.SAVE)

        self.pushButton_reload = QtWidgets.QPushButton(self)
        self.pushButton_reload.setGeometry(QtCore.QRect(970, 15, 20, 20))
        self.pushButton_reload.setFont(font)
        self.pushButton_reload.setText(msgs.RELOAD)
        self.pushButton_reload.clicked.connect(self.updateView)

        self.pushButton_trash = QtWidgets.QPushButton(self)
        self.pushButton_trash.setGeometry(QtCore.QRect(945, 15, 20, 20))
        self.pushButton_trash.setFont(font)
        self.pushButton_trash.setText(msgs.TRASH)
        self.pushButton_trash.setCheckable(True)
        self.pushButton_trash.clicked.connect(self.toggleDeleteList)

        self.lineEdit_serverip = QtWidgets.QLineEdit(self)
        self.lineEdit_serverip.setGeometry(QtCore.QRect(190, 250, 113, 20))
        self.lineEdit_serverip.setFont(font)
        self.lineEdit_password = QtWidgets.QLineEdit(self)
        self.lineEdit_password.setGeometry(QtCore.QRect(150, 400, 113, 20))
        self.lineEdit_password.setFont(font)

        self.check_encrypt = QtWidgets.QCheckBox(self)
        self.check_encrypt.setGeometry(QtCore.QRect(60, 360, 16, 17))
        self.check_encrypt.setFont(font)
        self.check_encrypt.setText("")

        # graphic view of screens
        self.graphicsView = GraphicView(self)

        # list of screens to delete
        self.deleteList = QListWidget(self)
        self.deleteList.setFixedSize(QtCore.QSize(100, 100))
        self.deleteList.move(863, 50)
        self.deleteList.hide()
        self.deleteList.itemDoubleClicked.connect(self.deleteFromList)

        self.startSections()
        self.startCryptSection()

        self.radioButton_server.toggled.connect(self.startSections)
        self.check_encrypt.toggled.connect(self.startCryptSection)

        # set settings occording to database
        if get_data(Settings.ENCRYPTION) == Settings.ENCRYPTION_ON:
            self.check_encrypt.setChecked(True)
            self.startCryptSection()

        if get_data(Settings.PC) == Settings.CLIENT:
            self.radioButton_client.setChecked(True)
            self.startSections()

        self.lineEdit_serverip.insert(get_data(Settings.IP))
        self.lineEdit_password.insert(get_data(Settings.PASS))

    def startSections(self):
        """
        Enable/Disable client and server sections.
        """
        if self.radioButton_client.isChecked():
            self.label_local.setDisabled(True)
            self.label_radioButton_server.setDisabled(True)
            self.label_radioButton_client.setDisabled(False)
            self.label_enterip.setDisabled(False)
            self.lineEdit_serverip.setDisabled(False)
            self.lineEdit_serverip.setDisabled(False)
            self.graphicsView.setDisabled(True)

        else:
            self.label_local.setDisabled(False)
            self.label_radioButton_server.setDisabled(False)
            self.label_radioButton_client.setDisabled(True)
            self.label_enterip.setDisabled(True)
            self.lineEdit_serverip.setDisabled(True)
            self.lineEdit_serverip.setDisabled(True)
            self.graphicsView.setDisabled(False)

    def startCryptSection(self):
        """
        Enable/Disable encryption section.
        """
        if self.check_encrypt.isChecked():
            self.label_encrypt.setDisabled(False)
            self.label_password.setDisabled(False)
            self.lineEdit_password.setDisabled(False)
        else:
            self.label_encrypt.setDisabled(True)
            self.label_password.setDisabled(True)
            self.lineEdit_password.setDisabled(True)

    def closeEvent(self, event):
        """
        Hides window on close event.
        """
        self.hide()
        event.ignore()

    def update(self):
        """
        Update database occording to set settings and screen locations.
        """
        set_data(Settings.IP, self.lineEdit_serverip.text())
        set_data(Settings.PASS, self.lineEdit_password.text())
        set_data(Settings.PC, int(self.radioButton_server.isChecked()))
        set_data(Settings.ENCRYPTION, int(self.check_encrypt.isChecked()))

        for screen_name in self.graphicsView.to_delete:
            remove_screen(screen_name)

        self.graphicsView.updateScreens()

    def onSave(self, func):
        """
        Sets save button press to given function.
        """
        self.pushButton_start.clicked.connect(func)

    def toggleDeleteList(self):
        """
        Toggles visibilty of delete list.
        """
        if self.pushButton_trash.isChecked():
            self.deleteList.show()
        else:
            self.deleteList.hide()

    def updateDeleteList(self):
        """
        Sets screens in delete list occording to screens in graphics view.
        """
        self.deleteList.clear()
        for i, screen_name in enumerate(self.graphicsView.screens):
            if screen_name != Server.NAME:
                self.deleteList.insertItem(i, screen_name)

    def deleteFromList(self, list_item):
        """
        Deletes screen from delete list and graphics view.
        """
        name = list_item.text()
        if name not in Server.machines:
            self.deleteList.takeItem(self.deleteList.row(list_item))
            self.graphicsView.deleteScreen(name)
    
    def updateView(self):
        """
        Updates the settings window.
        """
        self.label_local.setText(f"{msgs.IP_INF} {get_ip()}")
        self.graphicsView.load()
        self.updateDeleteList()

    def show(self):
        """
        Calls 'updateView' on show.
        """
        self.updateView()
        super().show()

    def connect(self, name):
        """
        Connects given screen name in the graphics view.
        """
        self.graphicsView.connectScreen(name)

    def disconnect(self, name):
        """
        Disconnects given screen name in the graphics view.
        """
        self.graphicsView.disconnectScreen(name)
