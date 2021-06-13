"""
Sqlite3 database used to store Settings options and machine locations.
"""
import sqlite3
import os

from src.files import DATABASE


class Settings:
    """
    Setting constants
    """
    PC = 'pc'
    PASS = 'password'
    IP = 'IP'
    ENCRYPTION = 'encryption'

    SERVER = 1
    CLIENT = 0
    ENCRYPTION_ON = 1
    ENCRYPTION_OFF = 0


class Screens:
    """
    Screen constants
    """
    TOP = 0
    BOTTOM = 3
    RIGHT = 1
    LEFT = 2

    @staticmethod
    def oppo(side):
        """
        Opposite side. ex: TOP -> BOTTOM, BOTTOM -> TOP
        f(x) = -x + 3
        """
        return -side + 3


# default settings
DEFAULTS = {
    Settings.IP: "",
    Settings.PASS: "",
    Settings.PC: Settings.SERVER,
    Settings.ENCRYPTION: Settings.ENCRYPTION_OFF
}


def sql_exec(*args, **kwargs):
    """
    Excecutes sqlite command.
    """
    conn = sqlite3.connect(DATABASE)
    c = conn.cursor()

    c.execute(*args, **kwargs)
    returned_data = c.fetchall()

    conn.commit()
    conn.close()
    return returned_data


def create():
    """
    Creates settings and screens table.
    """
    sql_exec(
        f'''CREATE TABLE settings (
                {Settings.IP} text,
                {Settings.PASS} text,
                {Settings.PC} integer,
                {Settings.ENCRYPTION} integer
        )'''
    )

    sql_exec("INSERT INTO settings VALUES (?, ?, ?, ?)",
             (
                 DEFAULTS[Settings.IP],
                 DEFAULTS[Settings.PASS],
                 DEFAULTS[Settings.PC],
                 DEFAULTS[Settings.ENCRYPTION],
             )
             )

    sql_exec(
        f'''CREATE TABLE screens (
                address text,
                top text,
                right text,
                bottom text,
                left text
        )'''
    )

    add_screen({Screens.LEFT: None, Screens.TOP: None, Screens.RIGHT: None, Screens.BOTTOM: None}, 'main')


def get_data(elem):
    """
    Gets given element from settings table.
    """
    return sql_exec(f"SELECT {elem} FROM settings")[0][0]


def set_data(elem, data):
    """
    Sets given element from settings table.
    """
    sql_exec(f"UPDATE settings SET {elem}=?", (data,))


def get_all_data():
    """
    Gets all data from settings table.
    """
    data = sql_exec("SELECT * from settings")[0]

    out = DEFAULTS.copy()
    for i, key in enumerate(out):
        out[key] = data[i]

    return out


def get_screen(name):
    """
    Gets screen from screens table.
    """
    try:
        screen = sql_exec(f"SELECT * FROM screens WHERE address=?", (name,))[0]
    except IndexError:
        return
    return screen


def get_attachments(name):
    """
    Gets attachments of given screen name.
    Adds screen with given name if not found.
    """
    screen = get_screen(name)
    if screen is None:
        attachments = {Screens.TOP: None, Screens.RIGHT: None, Screens.LEFT: None, Screens.BOTTOM: None}
        add_screen(attachments, name)
        # settings.update_view()
    else:
        attachments = {
            Screens.TOP: screen[1],
            Screens.RIGHT: screen[2],
            Screens.BOTTOM: screen[3],
            Screens.LEFT: screen[4]
        }
    return attachments


def get_screens():
    """
    Gets all screens from screens table.
    """
    screens = sql_exec("SELECT * from screens")
    return screens


def add_screen(attachments, name):
    """
    Adds screen to screens table.
    """
    sql_exec(
        "INSERT INTO screens VALUES (?, ?, ?, ?, ?)",
        (
            name,
            attachments[Screens.TOP],
            attachments[Screens.RIGHT],
            attachments[Screens.BOTTOM],
            attachments[Screens.LEFT]
        )
    )


def update_screen(attachments, address):
    """
    Updates screen from screens table.
    """
    sql_exec(
        "UPDATE screens SET top=?, right=?, bottom=?, left=? WHERE address=?",
        (
            attachments[Screens.TOP],
            attachments[Screens.RIGHT],
            attachments[Screens.BOTTOM],
            attachments[Screens.LEFT],
            address
        )
    )


def remove_screen(address):
    """
    Removes screen from screens table.
    """
    sql_exec(
        "DELETE from screens WHERE address=?",
        (
            address,
        )
    )


if not os.path.isfile(DATABASE):
    create()
