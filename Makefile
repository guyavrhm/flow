ifeq ($(OS),Windows_NT)

all: aes-win

aes-win: src/network/aes/aes.c src/network/aes/gmult.c
	gcc -fPIC -shared -o src/network/aes/aes.dll src/network/aes/aes.c src/network/aes/gmult.c

else

UNAME_S := $(shell uname -s)

all: aes-unix

aes-unix: src/network/aes/aes.c src/network/aes/gmult.c
	gcc -fPIC -shared -o src/network/aes/aes.so src/network/aes/aes.c src/network/aes/gmult.c

endif
