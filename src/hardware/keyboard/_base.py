from abc import ABC, abstractmethod

class BaseKeyboardController(ABC):
    @abstractmethod
    def press(self, key):
        pass

    @abstractmethod
    def release(self, key):
        pass


class BaseKeyboardListener(ABC):
    @abstractmethod
    def __init__(self, on_press=None, on_release=None, suppress=False):
        pass

    @abstractmethod
    def stop(self):
        pass
