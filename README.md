# flow
![GitHub release](https://img.shields.io/github/v/release/guyavrhm/flow)
![GitHub repo size](https://img.shields.io/github/languages/code-size/guyavrhm/flow)
![GitHub contributors](https://img.shields.io/github/contributors/guyavrhm/flow)
![GitHub licence](https://img.shields.io/github/license/guyavrhm/flow)

flow is a cross-platform virtual KVM software which allows control of multiple computers with multiple operating systems with one mouse and keyboard.

flow sends data through the local network, fast and securly, for you to have an effortless and cohesive experience. Simply move your mouse from one computer to another, flow will do all the work...

<br>

| Features | |
|-----------|-|
| Mouse and Keyboard Sharing | ✔️ |
| Clipboard Sharing | ✔️ |
| File Transfer | ✔️ |
| Unlimited Devices | ✔️ |
| Cross-platform | ✔️ |
| Set and Forget | ✔️ |
| AES Network Encryption | ✔️ |
| Zero Latency | ✔️ |
| Open-source | ✔️ |

<br>

## Installation

#### Source Code:
1. Download [python](https://www.python.org/downloads/release/python-395/). (>3.8)

2. `$ git clone https://github.com/guyavrhm/flow`

3. `$ pip install -r requirements.txt`
  
4. `$ make`

#### Binary Release:

1. Go to the [flow Releases Page](https://github.com/guyavrhm/flow/releases/latest).
2. Download the installer matching your operating system:
   * **Windows:** Download the `flow-windows-v*.setup.exe` installer and run it.
   * **Mac (Apple Silicon):** Download the `flow-macos-arm64-v*.dmg` file and drag flow to your Applications folder.
   * **Mac (Intel):** Download the `flow-macos-intel-v*.dmg` file and drag flow to your Applications folder.
   * **Linux:** Download the `flow-linux-v*.tar.gz` archive, extract it (`tar -xzf flow-linux-v*.tar.gz`), and run `./setup.sh`.


## Usage
1.
    #### Source Code:
    * `$ python flow.py`

    #### Binary Release:
    * Click on the flow application.

2. Simply move your mouse from one screen to the other, exactly like when having a second monitor.

#### Configuration

* While flow is running in the background, a tray icon will show.
<br>![image info](./img/tray.png)

* ![x](./img/x.png) indicates that there is no connection.

* ![v](./img/v.png) indicates that there is a connection.

* Right click on the icon to open the menu.
<br>![image info](./img/menu.png)<br>


## Contact
If you want to contact me you can reach me at my [email](mailto:guy.ava03@gmail.com).

## Other
Supported OSes:
* Windows
* MAC
* Linux (xorg)


Linux file renamed in python 3.9 fix:
```
cd /usr/lib/x86_64-linux-gnu/
ln -s -f libc.a liblibc.a
```

## License

Copyright (c) Guy Avraham. All rights reserved.

Licensed under the [MIT](LICENSE) license.
