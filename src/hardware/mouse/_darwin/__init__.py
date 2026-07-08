import logging
import threading
from Quartz import (
    CGEventCreate,
    CGEventGetLocation,
    CGWarpMouseCursorPosition,
    CGEventCreateMouseEvent,
    CGEventPost,
    kCGHIDEventTap,
    kCGEventMouseMoved,
    kCGEventLeftMouseDown,
    kCGEventLeftMouseUp,
    kCGEventRightMouseDown,
    kCGEventRightMouseUp,
    kCGEventOtherMouseDown,
    kCGEventOtherMouseUp,
    kCGEventLeftMouseDragged,
    kCGEventRightMouseDragged,
    kCGEventOtherMouseDragged,
    kCGEventScrollWheel,
    kCGMouseButtonLeft,
    kCGMouseButtonRight,
    kCGMouseButtonCenter,
    CGEventGetIntegerValueField,
    kCGMouseEventButtonNumber,
    kCGMouseEventDeltaX,
    kCGMouseEventDeltaY,
    kCGScrollWheelEventDeltaAxis1,
    kCGScrollWheelEventDeltaAxis2,
    CGEventCreateScrollWheelEvent,
    kCGScrollEventUnitLine,
    CGDisplayHideCursor,
    CGDisplayShowCursor,
    CGEventTapCreate,
    kCGSessionEventTap,
    kCGHeadInsertEventTap,
    kCGEventTapOptionDefault,
    CGEventMaskBit,
    CFRunLoopGetCurrent,
    CFRunLoopAddSource,
    kCFRunLoopDefaultMode,
    CFRunLoopRun,
    CFRunLoopStop,
    CGEventTapEnable,
    CFMachPortCreateRunLoopSource
)

from src.hardware.info import get_screeninfo
from src.hardware.mouse._base import BaseMouseController, BaseMouseListener

logger = logging.getLogger(__name__)


class MacOSMouseController(BaseMouseController):
    def __init__(self):
        self.pressed_buttons = {"left": False, "right": False, "middle": False, "x1": False, "x2": False}

    @property
    def position(self):
        event = CGEventCreate(None)
        point = CGEventGetLocation(event)
        return (point.x, point.y)

    @position.setter
    def position(self, pos):
        x, y = pos
        CGWarpMouseCursorPosition((x, y))

        if self.pressed_buttons["left"]:
            event_type = kCGEventLeftMouseDragged
            btn = kCGMouseButtonLeft
        elif self.pressed_buttons["right"]:
            event_type = kCGEventRightMouseDragged
            btn = kCGMouseButtonRight
        elif self.pressed_buttons["middle"]:
            event_type = kCGEventOtherMouseDragged
            btn = kCGMouseButtonCenter
        else:
            event_type = kCGEventMouseMoved
            btn = kCGMouseButtonLeft

        event = CGEventCreateMouseEvent(None, event_type, (x, y), btn)
        CGEventPost(kCGHIDEventTap, event)

    def press(self, button):
        if isinstance(button, str):
            button = button.lower()
        
        pos = self.position
        if button == "left":
            self.pressed_buttons["left"] = True
            event = CGEventCreateMouseEvent(None, kCGEventLeftMouseDown, pos, kCGMouseButtonLeft)
        elif button == "right":
            self.pressed_buttons["right"] = True
            event = CGEventCreateMouseEvent(None, kCGEventRightMouseDown, pos, kCGMouseButtonRight)
        elif button == "middle":
            self.pressed_buttons["middle"] = True
            event = CGEventCreateMouseEvent(None, kCGEventOtherMouseDown, pos, kCGMouseButtonCenter)
        elif button in ("x1", "button8"):
            self.pressed_buttons["x1"] = True
            event = CGEventCreateMouseEvent(None, kCGEventOtherMouseDown, pos, 3)
        elif button in ("x2", "button9"):
            self.pressed_buttons["x2"] = True
            event = CGEventCreateMouseEvent(None, kCGEventOtherMouseDown, pos, 4)
        else:
            logger.warning("MacOSMouseController: Unknown button press: %s", button)
            return

        CGEventPost(kCGHIDEventTap, event)

    def release(self, button):
        if isinstance(button, str):
            button = button.lower()

        pos = self.position
        if button == "left":
            self.pressed_buttons["left"] = False
            event = CGEventCreateMouseEvent(None, kCGEventLeftMouseUp, pos, kCGMouseButtonLeft)
        elif button == "right":
            self.pressed_buttons["right"] = False
            event = CGEventCreateMouseEvent(None, kCGEventRightMouseUp, pos, kCGMouseButtonRight)
        elif button == "middle":
            self.pressed_buttons["middle"] = False
            event = CGEventCreateMouseEvent(None, kCGEventOtherMouseUp, pos, kCGMouseButtonCenter)
        elif button in ("x1", "button8"):
            self.pressed_buttons["x1"] = False
            event = CGEventCreateMouseEvent(None, kCGEventOtherMouseUp, pos, 3)
        elif button in ("x2", "button9"):
            self.pressed_buttons["x2"] = False
            event = CGEventCreateMouseEvent(None, kCGEventOtherMouseUp, pos, 4)
        else:
            logger.warning("MacOSMouseController: Unknown button release: %s", button)
            return

        CGEventPost(kCGHIDEventTap, event)

    def scroll(self, dx, dy):
        event = CGEventCreateScrollWheelEvent(None, kCGScrollEventUnitLine, 2, dy, dx)
        CGEventPost(kCGHIDEventTap, event)


class MacOSMouseListener(BaseMouseListener, threading.Thread):
    def __init__(self, on_move=None, on_click=None, on_scroll=None, suppress=False):
        threading.Thread.__init__(self)
        self.on_move = on_move
        self.on_click = on_click
        self.on_scroll = on_scroll
        self.suppress = suppress
        self.runloop = None
        self.tap = None
        self.daemon = True

        if self.on_move:
            self.metrics = get_screeninfo()
            self.x_center = int(self.metrics[0] / 2)
            self.y_center = int(self.metrics[1] / 2)

    def run(self):
        self.runloop = CFRunLoopGetCurrent()
        
        if self.on_move:
            CGWarpMouseCursorPosition((self.x_center, self.y_center))
            CGDisplayHideCursor(0)

        mask = (
            CGEventMaskBit(kCGEventMouseMoved) |
            CGEventMaskBit(kCGEventLeftMouseDragged) |
            CGEventMaskBit(kCGEventRightMouseDragged) |
            CGEventMaskBit(kCGEventOtherMouseDragged) |
            CGEventMaskBit(kCGEventLeftMouseDown) |
            CGEventMaskBit(kCGEventLeftMouseUp) |
            CGEventMaskBit(kCGEventRightMouseDown) |
            CGEventMaskBit(kCGEventRightMouseUp) |
            CGEventMaskBit(kCGEventOtherMouseDown) |
            CGEventMaskBit(kCGEventOtherMouseUp) |
            CGEventMaskBit(kCGEventScrollWheel)
        )

        def callback(proxy, event_type, event, refcon):
            try:
                point = CGEventGetLocation(event)
                x, y = point.x, point.y

                # 1. Handle Movement / Dragging
                if self.on_move and event_type in (kCGEventMouseMoved, kCGEventLeftMouseDragged,
                                                   kCGEventRightMouseDragged, kCGEventOtherMouseDragged):
                    dx = CGEventGetIntegerValueField(event, kCGMouseEventDeltaX)
                    dy = CGEventGetIntegerValueField(event, kCGMouseEventDeltaY)

                    if dx != 0 or dy != 0:
                        CGWarpMouseCursorPosition((self.x_center, self.y_center))
                        self.on_move(dx, dy)
                    return None

                # 2. Handle Clicks
                elif event_type in (kCGEventLeftMouseDown, kCGEventLeftMouseUp,
                                    kCGEventRightMouseDown, kCGEventRightMouseUp,
                                    kCGEventOtherMouseDown, kCGEventOtherMouseUp):
                    pressed = event_type in (kCGEventLeftMouseDown, kCGEventRightMouseDown, kCGEventOtherMouseDown)
                    if event_type in (kCGEventLeftMouseDown, kCGEventLeftMouseUp):
                        btn = "Button.left"
                    elif event_type in (kCGEventRightMouseDown, kCGEventRightMouseUp):
                        btn = "Button.right"
                    else:
                        btn_num = CGEventGetIntegerValueField(event, kCGMouseEventButtonNumber)
                        if btn_num == 2:
                            btn = "Button.middle"
                        elif btn_num == 3:
                            btn = "Button.x1"
                        elif btn_num == 4:
                            btn = "Button.x2"
                        else:
                            btn = f"Button.button{btn_num}"

                    if self.on_click:
                        self.on_click(x, y, btn, pressed)

                # 3. Handle Scroll
                elif event_type == kCGEventScrollWheel:
                    dy = CGEventGetIntegerValueField(event, kCGScrollWheelEventDeltaAxis1)
                    dx = CGEventGetIntegerValueField(event, kCGScrollWheelEventDeltaAxis2)
                    if self.on_scroll:
                        self.on_scroll(x, y, dx, dy)

                if self.suppress:
                    return None
            except Exception as e:
                logger.error("Error in mouse event tap: %s", e)
            return event

        self.tap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionDefault,
            mask,
            callback,
            None
        )
        if not self.tap:
            logger.critical("Failed to create mouse event tap. Accessibility permission is required.")
            return

        source = CFMachPortCreateRunLoopSource(None, self.tap, 0)
        CFRunLoopAddSource(self.runloop, source, kCFRunLoopDefaultMode)
        CGEventTapEnable(self.tap, True)
        CFRunLoopRun()

    def stop(self):
        if self.tap:
            CGEventTapEnable(self.tap, False)
        if self.runloop:
            CFRunLoopStop(self.runloop)
        if self.on_move:
            CGDisplayShowCursor(0)
