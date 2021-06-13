import ctypes
import numpy as np

from src.files import AES_SO

class AES:
    """
    AES class allows functions written in aes.c 
    file (C language) to be used in python.
    """

    def __init__(self, key):

        lib = ctypes.cdll.LoadLibrary(AES_SO)

        # uint8_t *aes_init(uint8_t *key); 
        aes_init = lib.aes_init
        aes_init.restype = ctypes.POINTER(ctypes.c_uint8)

        # void aes_encrypt(uint8_t *data, uint8_t *k);
        self._aes_encrypt = lib.aes_encrypt
        self._aes_encrypt.restype = None

        # void aes_decrypt(uint8_t *cipher, uint8_t *k);
        self._aes_decrypt = lib.aes_decrypt
        self._aes_decrypt.restype = None

        # self._k is a pointer to the expanded key
        np_key = np.frombuffer(key, dtype=np.uint8)
        self._k = aes_init(ctypes.c_void_p(np_key.ctypes.data))

    def enc_block(self, data_16: bytes) -> bytes:
        """
        Encrypts 16 bytes of plaintext.
        """

        np_data = np.frombuffer(data_16, dtype=np.uint8)
        self._aes_encrypt(ctypes.c_void_p(np_data.ctypes.data), self._k)

        return np_data.tobytes()

    def dec_block(self, cipher_16: bytes) -> bytes:
        """
        Decrypts 16 bytes of ciphertext.
        """

        np_cipher = np.frombuffer(cipher_16, dtype=np.uint8)
        self._aes_decrypt(ctypes.c_void_p(np_cipher.ctypes.data), self._k)

        return np_cipher.tobytes()
