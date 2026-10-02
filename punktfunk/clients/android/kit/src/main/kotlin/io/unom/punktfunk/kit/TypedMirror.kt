package io.unom.punktfunk.kit

/**
 * The per-transition plane's change ledger, shared by every capture link ([DsCapture],
 * [Sc2Capture]): a parsed state goes out as button transitions and axis values ON CHANGE ONLY,
 * so a 250 Hz report rate does not become 250 Hz of identical wire events.
 */
internal class TypedMirror {
    private var wireButtons = 0
    private val lastAxis = IntArray(6) { Int.MIN_VALUE }

    /** Diff [buttons] (already the wire bitmask) and the six axes onto [p]. */
    fun push(
        p: GamepadRouter.ExternalPad,
        buttons: Int,
        lsX: Int,
        lsY: Int,
        rsX: Int,
        rsY: Int,
        lt: Int,
        rt: Int,
    ) {
        var changed = buttons xor wireButtons
        while (changed != 0) {
            val bit = changed and -changed // lowest changed bit
            p.button(bit, buttons and bit != 0)
            changed = changed and bit.inv()
        }
        wireButtons = buttons
        axis(p, Gamepad.AXIS_LS_X, lsX)
        axis(p, Gamepad.AXIS_LS_Y, lsY)
        axis(p, Gamepad.AXIS_RS_X, rsX)
        axis(p, Gamepad.AXIS_RS_Y, rsY)
        axis(p, Gamepad.AXIS_LT, lt)
        axis(p, Gamepad.AXIS_RT, rt)
    }

    private fun axis(p: GamepadRouter.ExternalPad, id: Int, v: Int) {
        if (lastAxis[id] == v) return
        lastAxis[id] = v
        p.axis(id, v)
    }
}
