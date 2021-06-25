#!/usr/bin/osascript
use framework "Appkit"

property this : a reference to current application
property NSScreen : a reference to NSScreen of this

property text item delimiters : linefeed

on run
    set resolutions to NSScreen's mainScreen's frame
    set resolution to item 2 of (resolutions as list) as text
end run
