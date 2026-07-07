#!/usr/bin/osascript
use framework "Appkit"

property this : a reference to current application
property NSPasteboardURLReadingFileURLsOnlyKey : a reference to NSPasteboardURLReadingFileURLsOnlyKey of this
property NSPasteboard : a reference to NSPasteboard of this
property NSDictionary : a reference to NSDictionary of this
property NSNumber : a reference to NSNumber of this
property NSArray : a reference to NSArray of this
property NSURL : a reference to NSURL of this

property pb : missing value
property text item delimiters : linefeed

on run

    set pb to NSPasteboard's generalPasteboard()
    set classss to NSArray's arrayWithObject:(NSURL's class)
    set opt to NSDictionary's dictionaryWithObject:(NSNumber's numberWithBool:yes) forKey:NSPasteboardURLReadingFileURLsOnlyKey

    set fs to pb's readObjectsForClasses:classss options:opt
    (result's valueForKey:"path") as list as text

end run
