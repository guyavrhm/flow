from PyQt5 import QtWidgets
from PyQt5.QtCore import Qt

from src.info.computerinfo import get_screeninfo


class ScreenBlocker(QtWidgets.QWidget):
    """
    Transparent QWidget used to hide mouse and block mouse press events.
    """

    def __init__(self):
        super().__init__()

        width, height = get_screeninfo()

        self.setGeometry(0, 0, width, height)
        self.setWindowFlags(
            Qt.WindowStaysOnTopHint |
            Qt.BypassWindowManagerHint |
            Qt.FramelessWindowHint |
            Qt.NoDropShadowWindowHint |
            Qt.ToolTip
        )

        self.setWindowOpacity(0.3)
        self.setGraphicsEffect(QtWidgets.QGraphicsOpacityEffect(self))
        self.setCursor(Qt.BlankCursor)
