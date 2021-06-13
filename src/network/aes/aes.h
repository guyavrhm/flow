#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>


/* Number of encryption/decryption rounds */
#define ROUNDS 10

/* Number of 32-bit (4-byte) words */
#define SIDE 4

/* Number of bytes in block */
#define SIZE 16


uint8_t *aes_init(uint8_t *key);

void aes_encrypt(uint8_t *data, uint8_t *k);

void aes_decrypt(uint8_t *cipher, uint8_t *k);
