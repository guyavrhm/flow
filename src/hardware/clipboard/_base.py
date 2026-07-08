from abc import ABC, abstractmethod


class BaseClipboard(ABC):
    """
    Abstract base class representing the clipboard interface.
    To support a new OS, inherit from this class and implement
    all the abstract methods.
    """

    @staticmethod
    @abstractmethod
    def set_files(files: list):
        """
        Sets given file paths to the clipboard.
        """
        raise NotImplementedError

    @staticmethod
    @abstractmethod
    def data():
        """
        Returns clipboard data.
        Could return a string (plain text) or a tuple of strings (file paths).
        """
        raise NotImplementedError

    @staticmethod
    @abstractmethod
    def set_text(data: str):
        """
        Sets plain text to the clipboard.
        """
        raise NotImplementedError
