import subprocess
import urllib.parse
import os

from src.hardware.clipboard._base import BaseClipboard


class LinuxClipboard(BaseClipboard):
    """
    Linux Clipboard api
    """

    @staticmethod
    def set_files(files: list):
        """
        Sets given file paths to the clipboard as a URI list.
        """
        if not files:
            return

        uris = []
        for file in files:
            abs_path = os.path.abspath(file)
            quoted_path = urllib.parse.quote(abs_path)
            uris.append(f"file://{quoted_path}")

        data = "\r\n".join(uris) + "\r\n"

        try:
            p = subprocess.Popen(['xclip', '-selection', 'clipboard', '-t', 'text/uri-list'], stdin=subprocess.PIPE)
            p.communicate(input=data.encode('utf-8'))
        except Exception:
            pass

    @staticmethod
    def data():
        """
        Returns clipboard data.
        """
        try:
            # Check supported targets first
            targets_out = subprocess.check_output(
                ['xclip', '-o', '-selection', 'clipboard', '-t', 'TARGETS'],
                stderr=subprocess.DEVNULL
            ).decode('utf-8', errors='replace')
            targets = [t.strip() for t in targets_out.splitlines()]
        except Exception:
            targets = []

        if 'text/uri-list' in targets:
            try:
                out_uris = subprocess.check_output(
                    ['xclip', '-o', '-selection', 'clipboard', '-t', 'text/uri-list'],
                    stderr=subprocess.DEVNULL
                ).decode('utf-8', errors='replace').strip()

                if out_uris:
                    # Convert URIs to file paths
                    lines = out_uris.splitlines()
                    paths = []
                    for line in lines:
                        line = line.strip()
                        if not line or line.startswith('#'):
                            continue
                        parsed = urllib.parse.urlparse(line)
                        if parsed.scheme == 'file' or not parsed.scheme:
                            path = urllib.parse.unquote(parsed.path)
                            if path:
                                paths.append(path)
                    if paths:
                        return tuple(paths)
            except Exception:
                pass

        # Fallback to standard plain text
        try:
            out = subprocess.check_output(
                ['xclip', '-o', '-selection', 'clipboard'],
                stderr=subprocess.DEVNULL
            ).decode('utf-8', errors='replace')
        except Exception:
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
