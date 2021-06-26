import os
import time
import shutil

import src.info.computerinfo as ci

from src.files import TEMP_FLOW
from src.hardware.clipboard import ClipboardListener, Clipboard
from src.ui.qtthread import flowThread


class VirtualClipboard:
    """
    Simulates a virtual clipboard between machines.

    TCP communication
    """

    if ci.platform == ci.WINDOWS:
        DIR_SLASH = '\\'
    else:
        DIR_SLASH = '/'

    def __init__(self):
        # clipboard event listener
        self._clipboard_listener = None
        # clipboard receiving thread
        self._receiving_t = None
        # weather data is received or changed manually
        self._received = False

        self._on = False

    def format_data(self, content):
        """
        Formats content recieved from clipboard:

        files (tuple) -> {type:file/folder, name:name, data(if file):raw} (dict)
        plain (str) -> plain (str)
        """
        if type(content) == tuple:

            root_dir = self.DIR_SLASH.join(content[0].split(self.DIR_SLASH)[:-1]) + self.DIR_SLASH
            files = []

            for item in content:
                if os.path.isfile(item):

                    with open(item, 'rb') as fi:
                        contents = fi.read()
                    files.append({'type': 'file', 'name': item, 'data': contents})

                elif os.path.isdir(item):
                    files.append({'type': 'folder', 'name': item})

                    for (dirpath, dirnames, filenames) in os.walk(item):
                        for d in dirnames:
                            files.append({'type': 'folder', 'name': dirpath + self.DIR_SLASH + d})

                        for f in filenames:
                            with open(dirpath + self.DIR_SLASH + f, 'rb') as fi:
                                contents = fi.read()
                            files.append({'type': 'file', 'name': dirpath + self.DIR_SLASH + f, 'data': contents})

            for f in files:
                f['name'] = f['name'].replace(root_dir, '').replace('\\', '/')

            return files

        else:
            return content

    def to_clip(self, content):
        """
        files (list) -> creates files in a temporary directory
        then copies them to the clipboard.

        plain text (str) -> copies text to the clipboard.
        """

        if os.path.isdir(TEMP_FLOW):
            shutil.rmtree(TEMP_FLOW)
        os.makedirs(TEMP_FLOW)

        if type(content) == list:

            for d in content:
                if d['type'] == 'folder':
                    os.makedirs(TEMP_FLOW + self.DIR_SLASH + d['name'])
                else:
                    with open(TEMP_FLOW + self.DIR_SLASH + d['name'], 'wb') as f:
                        f.write(d['data'])

            self._received = True
            Clipboard.set_files([TEMP_FLOW + self.DIR_SLASH + f for f in os.listdir(TEMP_FLOW)])

        else:
            self._received = True
            Clipboard.set_text(content)

    def on_change(self, clip_content):
        """
        Virtrual method
        On clipboard change event.
        """
        pass

    def receive(self):
        """
        Virtrual method
        Receives clipboard data from server/client.
        """
        pass

    def start(self):
        """
        Starts the clipboard event listener and the receiving thread.
        """
        self._on = True
        self._clipboard_listener = ClipboardListener(on_change=self.on_change)
        self._clipboard_listener.start()
        self._receiving_t = flowThread(target=self.receive)
        self._receiving_t.start()

    def stop(self):
        self._on = False
        if self._clipboard_listener is not None:
            self._clipboard_listener.stop()
            self._receiving_t.wait()


class ClientClipboard(VirtualClipboard):
    """
    Virtual clipboard used by client.
    """

    def __init__(self, client):
        super(ClientClipboard, self).__init__()
        # client class
        self.client = client

    def receive(self):
        """
        Receives clipboard contents from server.
        Sets clipboard to contents.
        """
        try:
            while self._on:

                try:
                    content = self.client.tcp_sock.true_recv()
                except ValueError:
                    # server closed content b''
                    self.client.reconnect()
                    continue

                self.to_clip(content)
                time.sleep(1)
        except (ConnectionAbortedError, OSError):
            # manualy disconnected
            return

    def on_change(self, clip_content):
        """
        Sends clipboard data to server on clipboard change.
        """
        try:
            if not self._received:
                formatted_content = self.format_data(clip_content)
                self.client.tcp_sock.true_send(formatted_content)

            self._received = False

        except OSError:
            # when socket closes before initialized
            pass


class ServerClipboard(VirtualClipboard):
    """
    Virtual clipboard used by server.
    """

    def __init__(self, server):
        super(ServerClipboard, self).__init__()
        # server class
        self.server = server

    def receive(self):
        """
        Receives clipboard contents from server.
        Sets clipboard to contents and sends content to all clients.
        """
        try:
            while self._on:

                content = None
                sent_from = None
                for m in list(self.server.machines.values())[1:]:
                    try:
                        content = m.tcp_conn.true_recv()
                        sent_from = m
                    except BlockingIOError:
                        # No data to read
                        pass
                    except (ConnectionResetError, ValueError):
                        # Client disconnected
                        self.server.remove_client(m)

                if content is not None:
                    self.to_clip(content)

                    for m in list(self.server.machines.values())[1:]:
                        if m != sent_from:
                            m.tcp_conn.true_send(content)

                time.sleep(1)
        except OSError:
            return

    def on_change(self, clip_content):
        """
        Sends clipboard content to all connected clients on clibpoard change.
        """
        try:
            if not self._received:
                formatted_content = self.format_data(clip_content)

                for m in list(self.server.machines.values())[1:]:
                    m.tcp_conn.true_send(formatted_content)

            self._received = False
        except OSError:
            pass
