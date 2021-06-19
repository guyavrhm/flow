import math

from PyQt5 import QtCore, QtGui, QtWidgets
from PyQt5.QtCore import Qt, QPointF
from PyQt5.QtGui import QPen, QColor
from PyQt5.QtWidgets import QGraphicsView, QGraphicsScene, QGraphicsRectItem

from .components import app

from src.data.db import get_screens, update_screen, Screens
from src.comms.server import Server

# main screen location information
MAIN_X = 198
MAIN_Y = 190
MAIN_WIDTH = 195
MAIN_HEIGHT = 136

MAIN_BOTTOM = 326  # Y + HEIGHT
MAIN_TOP = 54  # Y - HEIGHT
MAIN_LEFT = 3  # X - WIDTH
MAIN_RIGHT = 393  # X + WIDTH

MAIN_CENTER = (295, 258)


class MovingScreen(QGraphicsRectItem):
    """
    QGraphicsRectItem with drag and drop.
    """

    # colors
    GREEN = QColor(159, 230, 160)
    GREY = QColor(205, 208, 203)

    # size
    DEFAULT_HIEGHT = 136
    DEFAULT_WIDTH = 192

    def __init__(self, name, x, y, w, h, fill, outline, attachments, view):
        super().__init__(0, 0, w, h)

        # every screen is aware of the whole view
        self.view = view

        self.name = name
        # text shown on body
        self.text = QtWidgets.QGraphicsSimpleTextItem(self.name, self)
        self.text.setFont(self.view.font)
        self.text.setPos(5, 5)

        # position
        self.setPos(x, y)
        self.h = h
        self.w = w

        self.setBrush(fill)
        self.setPen(outline)
        self.setAcceptHoverEvents(True)

        # attachments to other screens
        self.attachments = attachments

    def hoverEnterEvent(self, event):
        """
        Sets mouse cursor to an open hand on hover.
        """
        app.instance().setOverrideCursor(Qt.OpenHandCursor)

    def hoverLeaveEvent(self, event):
        """
        Sets mouse cursor to default on leaving hover.
        """
        app.instance().setOverrideCursor(Qt.ArrowCursor)

    def mousePressEvent(self, event):
        """
        Sets mouse cursor to closed hand on press.
        """
        app.instance().setOverrideCursor(Qt.ClosedHandCursor)
        self.setZValue(1)

    def mouseMoveEvent(self, event):
        """
        Moves screen with mouse.
        """
        orig_cursor_position = event.lastScenePos()
        updated_cursor_position = event.scenePos()
        orig_position = self.scenePos()

        updated_position_x = updated_cursor_position.x() - orig_cursor_position.x() + orig_position.x()
        updated_position_y = updated_cursor_position.y() - orig_cursor_position.y() + orig_position.y()
        self.setPos(QPointF(updated_position_x, updated_position_y))

    def mouseReleaseEvent(self, event):
        """
        Snaps released screen and updates attachments.
        """
        app.instance().setOverrideCursor(Qt.OpenHandCursor)
        self.snap()
        self.setZValue(0)
        self.view.updateAttachments()

    def centerX(self):
        return (self.pos().x() + (self.pos().x() + self.w)) / 2

    def centerY(self):
        return (self.pos().y() + (self.pos().y() + self.h)) / 2

    def rightX(self):
        return self.pos().x() + self.w

    def leftX(self):
        return self.pos().x()

    def topY(self):
        return self.pos().y()

    def bottomY(self):
        return self.pos().y() + self.h

    def closest(self):
        """
        Returns closest screen to current screen.
        """
        min_distance_screen = None

        for test_screen in self.view.screens.values():
            if self is not test_screen:

                if min_distance_screen is None:
                    min_distance_screen = test_screen
                elif self.distance(test_screen) < self.distance(min_distance_screen):
                    min_distance_screen = test_screen

        return min_distance_screen

    def distance(self, other_screen):
        """
        Returns distance of centers of current and given screen.
        """
        return math.sqrt((other_screen.centerX() - self.centerX()) ** 2 +
                         (other_screen.centerY() - self.centerY()) ** 2)

    def snap(self):
        """
        Snaps current screen to closest screen.
        """
        closest_screen = self.closest()
        if closest_screen is None:
            return
        relative_x = self.centerX() - closest_screen.centerX()
        relative_y = self.centerY() - closest_screen.centerY()

        if abs(relative_x) > abs(relative_y):
            if relative_x > 0:
                pos = Screens.LEFT
            else:
                pos = Screens.RIGHT
        else:
            if relative_y > 0:
                pos = Screens.TOP
            else:
                pos = Screens.BOTTOM

        self.snapTo(closest_screen, pos)

    def snapTo(self, screen, side):
        """
        Snaps to specific side of given screen.
        """
        if side == Screens.LEFT:
            self.setPos(screen.rightX(), screen.pos().y())
        elif side == Screens.RIGHT:
            self.setPos(screen.leftX() - self.w, screen.pos().y())
        elif side == Screens.TOP:
            self.setPos(screen.pos().x(), screen.bottomY())
        elif side == Screens.BOTTOM:
            self.setPos(screen.pos().x(), screen.topY() - self.h)

    def getCenter(self):
        """
        Returns center of current screen.
        """
        y_center = (self.pos().y() + (self.pos().y() + self.h)) / 2
        x_center = (self.pos().x() + (self.pos().x() + self.w)) / 2
        return int(x_center), int(y_center)

    def callToSnap(self, ignore=None):
        """
        Recursive call screens from attachments to snap to calling screen.
        """
        if ignore is None:
            ignore = []
        for side in self.attachments:
            name_to_call = self.attachments[side]
            if name_to_call is not None and name_to_call not in ignore:
                screen = self.view.screens[name_to_call]
                screen.snapTo(self, Screens.oppo(side))
                ignore.append(self.name)
                screen.callToSnap(ignore)


class GraphicView(QGraphicsView):
    """
    QGraphicsView which contains the MovingScreen(s)
    """

    def __init__(self, widget):
        super().__init__(widget)

        self.scene = QGraphicsScene()
        self.setScene(self.scene)
        self.setSceneRect(0, 0, 600, 531)
        self.setGeometry(QtCore.QRect(390, 40, 600, 531))
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)

        # font
        self.font = QtGui.QFont()
        self.font.setPixelSize(15)

        # pens
        self.blackPen = QPen(Qt.black)
        self.blackPen.setWidth(3)
        self.grayPen = QPen(QColor(199, 199, 199))
        self.grayPen.setWidth(3)

        # screens in database represented as MovingScreen(s)
        self.screens = {}
        # list of screen names to delete
        self.to_delete = []

    def addScreens(self):
        """
        Adds screens from database to view.
        """
        db_screens = get_screens()

        for db_screen in db_screens:
            new_screen = self.dbScreenToScreen(db_screen)
            self.screens[new_screen.name] = new_screen
            self.scene.addItem(new_screen)

    def dbScreenToScreen(self, db_screen):
        """
        Converts screen from database to graphical screen.
        """
        name = db_screen[0]

        if name == Server.NAME:
            # color = Qt.lightGray
            color = MovingScreen.GREEN
            x = MAIN_X
            y = MAIN_Y
        else:
            if name in Server.machines:
                color = MovingScreen.GREEN
            else:
                color = MovingScreen.GREY
            x = 0
            y = 0

        attachments_list = db_screen[1:]
        attachments_dict = {
            Screens.TOP: attachments_list[0],
            Screens.RIGHT: attachments_list[1],
            Screens.BOTTOM: attachments_list[2],
            Screens.LEFT: attachments_list[3]
        }
        return MovingScreen(name, x, y, MovingScreen.DEFAULT_WIDTH, MovingScreen.DEFAULT_HIEGHT, color, self.blackPen,
                            attachments_dict, self)

    def updateAttachments(self):
        """
        Updates all screens' attachments occording to their position.
        """
        for s in self.screens.values():
            s.attachments = {Screens.TOP: None, Screens.RIGHT: None, Screens.LEFT: None, Screens.BOTTOM: None}
            for test_screen in self.screens.values():
                if s is not test_screen:
                    if s.leftX() == test_screen.rightX() and s.pos().y() == test_screen.pos().y():
                        s.attachments[Screens.LEFT] = test_screen.name
                    elif s.rightX() == test_screen.leftX() and s.pos().y() == test_screen.pos().y():
                        s.attachments[Screens.RIGHT] = test_screen.name
                    elif s.topY() == test_screen.bottomY() and s.pos().x() == test_screen.pos().x():
                        s.attachments[Screens.TOP] = test_screen.name
                    elif s.bottomY() == test_screen.topY() and s.pos().x() == test_screen.pos().x():
                        s.attachments[Screens.BOTTOM] = test_screen.name

    def connectScreen(self, name):
        """
        Changes color of given screen to green.
        """
        try:
            self.screens[name].setBrush(MovingScreen.GREEN)
        except KeyError:
            pass

    def disconnectScreen(self, name):
        """
        Changes color of given screen to grey.
        """
        try:
            self.screens[name].setBrush(MovingScreen.GREY)
        except KeyError:
            pass

    def load(self):
        """
        Reloads viev.
        """
        self.scene.clear()
        self.screens.clear()
        self.to_delete.clear()

        self.addScreens()
        self.screens[Server.NAME].callToSnap()

    def deleteScreen(self, name):
        """
        Temporarly remove screen from view,
        will completely delete from database on save.
        """
        for s in self.screens.values():
            if name in s.attachments.values():
                loc = list(s.attachments.keys())[list(s.attachments.values()).index(name)]
                s.attachments[loc] = None

        self.scene.removeItem(self.screens[name])
        del self.screens[name]
        self.to_delete.append(name)

    def updateScreens(self):
        """
        Updates screens in database and attachments of current connected machines.
        """
        for s in self.screens.values():
            update_screen(s.attachments, s.name)
            try:
                Server.machines[s.name].attachments = s.attachments
            except KeyError:
                pass
