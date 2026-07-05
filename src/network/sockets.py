"""
Adds encryption and better functionality to socket.socket.
"""
import socket
import pickle

from .encryption import Encryption

# maximum digits of data to send
LENGTH = 10
# size of data to receive (bytes)
BUFFER = 1024

# encryption key
key = Encryption(b'')


class DifferentEncryption(Exception):
    pass


def set_encryption_key(password: str):
    return Encryption(password.encode())


def true_accept(sock):
    """
    Accepts new client.

    :raises DifferentEncryption: if client doesn't have
    the same encryption password as this socket.
    """
    c, a = sock.accept()
    try:
        c.true_recv()
        c.true_send('.')
    except (EOFError, ValueError, pickle.UnpicklingError):
        c.true_send('.')
        raise DifferentEncryption from None

    return c, a


def true_connect(sock, address):
    """
    Connects to a server listening on given address.

    :raises DifferentEncryption: if server doesn't have 
    the same encryption password as this socket.
    """
    sock.connect(address)

    sock.true_send('.')
    try:
        sock.true_recv()
    except (EOFError, ValueError, pickle.UnpicklingError):
        raise DifferentEncryption from None


def recv_exactly(conn, n):
    """
    Receives exactly n bytes from connection.
    Raises ConnectionError if connection closes prematurely.
    """
    data = b''
    while len(data) < n:
        packet = conn.recv(n - len(data))
        if not packet:
            raise ConnectionError("Socket closed prematurely")
        data += packet
    return data


def true_send(conn, data):
    """
    Sends encrypted data to connection (TCP).
    """
    encrypted_data = key.encrypt(pickle.dumps(data))
    length = str(len(encrypted_data)).zfill(LENGTH).encode()
    data = length + encrypted_data
    conn.sendall(data)


def true_recv(conn):
    """
    Receives all encrypted data from connection (TCP).
    """
    length = int(recv_exactly(conn, LENGTH))
    data = recv_exactly(conn, length)
    return pickle.loads(key.decrypt(data))


def true_sendto(conn, data, address, special=False):
    """
    Sends encrypted data to given address (UDP).

    :param special: allow data not only in bytes
    """
    if special:
        data = pickle.dumps(data)
    else:
        data = data.encode()
    conn.sendto(key.encrypt(data), (address[0], address[1]))  # (ip_dst, dport)


def true_recvfrom(conn, buff):
    """
    Receives encrypted data (size of buff) from connection (UDP).
    """
    received, address = conn.recvfrom(buff)
    data = key.decrypt(received)
    try:
        return data.decode(), address
    except UnicodeDecodeError:  # pickle data
        return pickle.loads(data), address


# set socket.socket's new attributes
setattr(socket.socket, 'true_send', true_send)
setattr(socket.socket, 'true_recv', true_recv)
setattr(socket.socket, 'true_sendto', true_sendto)
setattr(socket.socket, 'true_recvfrom', true_recvfrom)
setattr(socket.socket, 'true_accept', true_accept)
setattr(socket.socket, 'true_connect', true_connect)
