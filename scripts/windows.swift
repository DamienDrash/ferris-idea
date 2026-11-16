import CoreGraphics
// Window counter for startup.sh: reads only the public window list (CGWindowList), no accessibility rights.
// Build: swiftc -O -o evidence/bin/windows scripts/windows.swift
import Foundation
// prints "<count> <maxWidth>x<maxHeight>" of on-screen layer-0 windows owned by the given pid
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
var n = 0; var mw = 0; var mh = 0
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
  let b = w[kCGWindowBounds as String] as! [String: Any]
  let ww = (b["Width"] as! NSNumber).intValue, hh = (b["Height"] as! NSNumber).intValue
  n += 1; if ww * hh > mw * mh { mw = ww; mh = hh }
}
print("\(n) \(mw)x\(mh)")
