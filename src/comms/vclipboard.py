import os
import time
import shutil
import logging
import threading
from collections import deque

import src.hardware.info as ci

from src.files import TEMP_FLOW
from src.hardware.clipboard import ClipboardListener, Clipboard
from src.ui.qtthread import flowThread

logger = logging.getLogger(__name__)


class VirtualClipboard:
    """
    Simulates a virtual clipboard between machines.

    TCP communication
    """

    DIR_SLASH = os.sep

    def __init__(self):
        # clipboard event listener
        self._clipboard_listener = None
        # clipboard receiving thread
        self._receiving_t = None
        # Thread-safe history of updates received from the network to prevent feedback loops
        self._history_lock = threading.Lock()
        self._received_history = deque(maxlen=3)

        self._on = False

    def format_data(self, content):
        """
        Formats content recieved from clipboard:

        files (tuple) -> {type:file/folder, name:name, data(if file):raw} (dict)
        plain (str) -> plain (str)
        """
        if type(content) == tuple:
            logger.info("Formatting %d clipboard files/folders for transmission", len(content))
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
            logger.info("Writing %d formatted files/folders from network to local clipboard", len(content))
            for d in content:
                if d['type'] == 'folder':
                    os.makedirs(TEMP_FLOW + self.DIR_SLASH + d['name'])
                else:
                    with open(TEMP_FLOW + self.DIR_SLASH + d['name'], 'wb') as f:
                        f.write(d['data'])

            # Normalize files to a sorted tuple of top-level paths under TEMP_FLOW
            file_paths = tuple(sorted([TEMP_FLOW + self.DIR_SLASH + f for f in os.listdir(TEMP_FLOW)]))
            with self._history_lock:
                self._received_history.append(file_paths)
            Clipboard.set_files(list(file_paths))

        else:
            text_preview = content[:50] + "..." if len(content) > 50 else content
            logger.info("Writing text data to local clipboard: '%s'", text_preview)
            with self._history_lock:
                self._received_history.append(content)
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
        Starts the clipboard event listener.
        """
        logger.info("Starting virtual clipboard listener")
        self._on = True
        self._clipboard_listener = ClipboardListener(on_change=self.on_change, parent=None)
        self._clipboard_listener.start()

    def stop(self):
        logger.info("Stopping virtual clipboard listener")
        self._on = False
        if self._clipboard_listener is not None:
            try:
                self._clipboard_listener.stop()
                self._clipboard_listener.wait()
                self._clipboard_listener.deleteLater()
            except Exception as e:
                logger.debug("Failed to clean up clipboard listener: %s", e)
            self._clipboard_listener = None
        if self._receiving_t is not None:
            try:
                self._receiving_t.wait()
                self._receiving_t.deleteLater()
            except Exception as e:
                logger.debug("Failed to clean up clipboard receiver thread: %s", e)
            self._receiving_t = None


class ClientClipboard(VirtualClipboard):
    """
    Virtual clipboard used by client.
    """

    def __init__(self, client):
        super(ClientClipboard, self).__init__()
        # client class
        self.client = client

    def start(self):
        """
        Starts the clipboard event listener and the client receiving thread.
        """
        super(ClientClipboard, self).start()
        self._receiving_t = flowThread(target=self.receive, parent=None)
        self._receiving_t.start()

    def receive(self):
        """
        Receives clipboard contents from server.
        Sets clipboard to contents.
        """
        logger.info("Client clipboard receiver loop started")
        while self._on:
            try:
                content = self.client.tcp_sock.true_recv()
                if not self._on:
                    break
                if content is not None:
                    logger.info("Received clipboard update from server")
                    self.to_clip(content)
            except Exception as e:
                if not self._on:
                    break
                logger.warning("Error receiving server clipboard update: %s. Initiating reconnect.", e)
                self.client.reconnect()
                continue

    def on_change(self, clip_content):
        """
        Sends clipboard data to server on clipboard change.
        """
        try:
            if isinstance(clip_content, (list, tuple)):
                normalized_content = tuple(sorted(clip_content))
            else:
                normalized_content = clip_content

            is_received = False
            with self._history_lock:
                if normalized_content in self._received_history:
                    is_received = True
                    try:
                        self._received_history.remove(normalized_content)
                    except ValueError:
                        pass

            if not is_received:
                logger.info("Local clipboard changed; sending update to server")
                formatted_content = self.format_data(clip_content)
                self.client.tcp_sock.true_send(formatted_content)
            else:
                logger.info("Ignoring clipboard change; matched network received update")

        except Exception as e:
            logger.debug("Exception in client clipboard on_change: %s", e)
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
        client_name = machine.address[0] if machine.address else "Unknown"
        logger.info("Starting clipboard receiver thread for client machine: %s", client_name)
        t = flowThread(target=lambda: self.receive_from_client(machine), parent=None)
        machine.clipboard_thread = t
        t.start()

    def receive_from_client(self, machine):
        """
        Dedicated blocking loop for receiving clipboard content from a specific client.
        """
        client_name = machine.address[0] if machine.address else "Unknown"
        logger.info("Started receiver loop for client %s clipboard", client_name)
        while self._on:
            try:
                content = machine.tcp_conn.true_recv()
                if not self._on:
                    break
                if content is not None:
                    logger.info("Received clipboard update from client %s", client_name)
                    self.to_clip(content)
                    
                    # Broadcast to all other machines
                    with self.server.machines_lock:
                        machines_list = list(self.server.machines.values())[1:]
                    logger.info("Broadcasting clipboard update to %d other clients", len(machines_list) - 1)
                    for m in machines_list:
                        if m != machine:
                            try:
                                logger.info("Sending broadcast clipboard update to client: %s", m.address[0] if m.address else "Unknown")
                                m.tcp_conn.true_send(content)
                            except OSError as oe:
                                logger.debug("Failed to send broadcast to %s: %s", m.address[0] if m.address else "Unknown", oe)
                                pass
            except Exception as e:
                if not self._on:
                    break
                logger.warning("Exception in clipboard receiver for client %s: %s. Removing client.", client_name, e)
                self.server.remove_client(machine)
                break

    def on_change(self, clip_content):
        """
        Sends clipboard content to all connected clients on clipboard change.
        """
        try:
            if isinstance(clip_content, (list, tuple)):
                normalized_content = tuple(sorted(clip_content))
            else:
                normalized_content = clip_content

            is_received = False
            with self._history_lock:
                if normalized_content in self._received_history:
                    is_received = True
                    try:
                        self._received_history.remove(normalized_content)
                    except ValueError:
                        pass

            if not is_received:
                logger.info("Server local clipboard changed; broadcasting to all clients")
                formatted_content = self.format_data(clip_content)

                with self.server.machines_lock:
                    machines_list = list(self.server.machines.values())[1:]
                logger.info("Broadcasting updated clipboard to %d clients", len(machines_list))
                for m in machines_list:
                    try:
                        logger.info("Broadcasting clipboard update to client: %s", m.address[0] if m.address else "Unknown")
                        m.tcp_conn.true_send(formatted_content)
                    except Exception as e:
                        logger.debug("Failed to broadcast clipboard update to %s: %s", m.address[0] if m.address else "Unknown", e)
                        pass
            else:
                logger.info("Ignoring server clipboard change; matched network received update")

        except Exception as e:
            logger.debug("Exception in server clipboard on_change: %s", e)
            pass

