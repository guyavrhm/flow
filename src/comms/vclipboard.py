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
        parent = getattr(self, 'client', getattr(self, 'server', None))
        self._clipboard_listener = ClipboardListener(on_change=self.on_change, parent=parent)
        self._clipboard_listener.start()
        if self.__class__.__name__ != 'ServerClipboard':
            self._receiving_t = flowThread(target=self.receive, parent=parent)
            self._receiving_t.start()

    def stop(self):
        self._on = False
        if self._clipboard_listener is not None:
            try:
                self._clipboard_listener.stop()
                self._clipboard_listener.deleteLater()
            except Exception:
                pass
            self._clipboard_listener = None
        if self._receiving_t is not None:
            try:
                self._receiving_t.wait()
                self._receiving_t.deleteLater()
            except Exception:
                pass
            self._receiving_t = None


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
        while self._on:
            try:
                content = self.client.tcp_sock.true_recv()
                if not self._on:
                    break
                if content is not None:
                    self.to_clip(content)
            except Exception:
                if not self._on:
                    break
                self.client.reconnect()
                continue

    def on_change(self, clip_content):
        """
        Sends clipboard data to server on clipboard change.
        """
        try:
            if not self._received:
                formatted_content = self.format_data(clip_content)
                self.client.tcp_sock.true_send(formatted_content)

            self._received = False

        except Exception:
            # when socket closes before initialized or other socket errors
            pass


class ServerClipboard(VirtualClipboard):
    """
    Virtual clipboard used by server.
    """

    def __init__(self, server):
        super(ServerClipboard, self).__init__()
        # server class
        self.server = server

    def start_client_receiver(self, machine):
        """
        Starts a dedicated clipboard receiver thread for the given client machine.
        """
        t = flowThread(target=lambda: self.receive_from_client(machine), parent=self.server)
        machine.clipboard_thread = t
        t.start()

    def receive_from_client(self, machine):
        """
        Dedicated blocking loop for receiving clipboard content from a specific client.
        """
        while self._on:
            try:
                content = machine.tcp_conn.true_recv()
                if not self._on:
                    break
                if content is not None:
                    self.to_clip(content)
                    
                    # Broadcast to all other machines
                    with self.server.machines_lock:
                        machines_list = list(self.server.machines.values())[1:]
                    for m in machines_list:
                        if m != machine:
                            try:
                                m.tcp_conn.true_send(content)
                            except OSError:
                                pass
            except Exception:
                if not self._on:
                    break
                self.server.remove_client(machine)
                break

    def on_change(self, clip_content):
        """
        Sends clipboard content to all connected clients on clipboard change.
        """
        try:
            if not self._received:
                formatted_content = self.format_data(clip_content)

                with self.server.machines_lock:
                    machines_list = list(self.server.machines.values())[1:]
                for m in machines_list:
                    try:
                        m.tcp_conn.true_send(formatted_content)
                    except Exception:
                        pass

            self._received = False
        except Exception:
            pass
