import subprocess



class LinuxClipboard:
    """
    Linux Clipboard api
    """

    @staticmethod
    def set_files(files: list):
        """
        file sharing not supported on linux
        """
        LinuxClipboard.set_text(str(files))

    @staticmethod
    def data():
        """
        Returns clipboard data.
        """
        try:
            out = subprocess.check_output(['xclip', '-o', '-selection', 'clipboard'], stderr=subprocess.DEVNULL).decode('utf-8')
        except subprocess.CalledProcessError:
            out = 'unknown format'
        return out

    @staticmethod
    def set_text(data: str):
        """
        Sets plain text to the clipboard.
        """
        try:
            p = subprocess.Popen(['xclip', '-selection', 'clipboard'], stdin=subprocess.PIPE)
            p.communicate(input=data.encode('utf-8'))
        except Exception:
            pass
