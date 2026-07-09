#!/usr/bin/osascript
use framework "Appkit"

property this : a reference to current application
property NSMutableArray : a reference to NSMutableArray of this
property NSPasteboard : a reference to NSPasteboard of this
property NSString : a reference to NSString of this
property NSURL : a reference to NSURL of this

property pb : missing value

on run input
    init()
    clearClipboard()
    addToClipboard(input)
end run

to init()
    set pb to NSPasteboard's generalPasteboard()
end init

to clearClipboard()
    pb's clearContents()
end clearClipboard

to addToClipboard(fs)
    set fURLs to NSMutableArray's array()

    repeat with f in fs
        set pathStr to contents of f
        set fp to (NSString's stringWithString:pathStr)'s ¬
            stringByStandardizingPath()
        fURLs's addObject:(NSURL's fileURLWithPath:fp)
    end repeat

    log ("Count of fURLs: " & (fURLs's |count|() as string))
    pb's writeObjects:fURLs
end addToClipboard
