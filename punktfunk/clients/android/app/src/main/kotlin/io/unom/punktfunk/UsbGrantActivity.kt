package io.unom.punktfunk

import android.app.Activity
import android.os.Bundle

/**
 * The USB_DEVICE_ATTACHED target. Matching `res/xml/usb_device_filter.xml` is what persists a
 * pad's USB permission, and on Android TV it is the only route that does: the in-app dialog
 * frequently never appears there. It finishes at once — [MainActivity]'s attach receiver and
 * `onResume` are what pick the grant up and start the capture.
 */
class UsbGrantActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        finish()
    }
}
