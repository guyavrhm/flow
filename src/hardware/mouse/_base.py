from abc import ABC, abstractmethod

class BaseMouseController(ABC):
    @property
    @abstractmethod
    def position(self):
        pass

    @position.setter
    @abstractmethod
    def position(self, pos):
        pass

    @abstractmethod
    def press(self, button):
        pass

    @abstractmethod
    def release(self, button):
        pass

    @abstractmethod
    def scroll(self, dx, dy):
        pass


class BaseMouseListener(ABC):
    @abstractmethod
    def __init__(self, on_move=None, on_click=None, on_scroll=None, suppress=False):
        pass

    @abstractmethod
    def stop(self):
        pass
