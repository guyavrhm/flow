import hashlib
from .aes import AES


class Encryption:
    """
    AES wrapper class. Allows encryption/decryption
    of any byte size data.
    """

    BLOCK_SIZE = 16
    PADDING_BYTE = b'\x00'  # Additional byte added to complete 128-bit data

    def __init__(self, password: bytes):

        if len(password) != 0:
            try:
                hashed_key = hashlib.md5(password).digest()
            except TypeError:
                raise TypeError("a bytes-like object is required, not 'str'") from None

            self._key = AES(hashed_key)

        else:
            self._key = ''

    def encrypt(self, data: bytes) -> bytes:
        """
        Encrypts any byte size data.
        Adds padding bytes to complete data devisible by 16.

        this text will be encrypted ->
        \x04this text will be encrypted\x00\x00\x00\x00 ->
        $hsj76*6&gdf#$4dkr47ns^jd@!fsj(7
        """

        if self._key == '':
            return data

        cipher = b''
        padding_len = self.BLOCK_SIZE - (len(data) % self.BLOCK_SIZE) - 1
        blocks = bytes([padding_len]) + data + bytes(padding_len)

        while len(blocks) > 0:
            cipher += self._key.enc_block(blocks[0:self.BLOCK_SIZE])
            blocks = blocks[self.BLOCK_SIZE:]

        return cipher

    def decrypt(self, cipher: bytes) -> bytes:
        """
        Decrypts padded data.
        Removes the first byte from the decrypted data and 
        the number of bytes shown on the first byte from the end.

        $hsj76*6&gdf#$4dkr47ns^jd@!fsj(7 ->
        \x04this text will be encrypted\x00\x00\x00\x00 ->
        this text will be encrypted
        """

        if self._key == '':
            return cipher

        plain = b''

        while len(cipher) > 0:
            plain += self._key.dec_block(cipher[0:self.BLOCK_SIZE])
            cipher = cipher[self.BLOCK_SIZE:]

        if plain[0] == 0:
            return plain[1:]
        else:
            return plain[1:-plain[0]]
