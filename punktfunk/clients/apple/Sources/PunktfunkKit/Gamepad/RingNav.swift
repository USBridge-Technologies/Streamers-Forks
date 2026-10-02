/// What a pad does to the in-stream quick-action ring while the ring owns the pad
/// (design/touch-client-overlay.md §2.6): the D-pad steps the highlight, the left stick aims it,
/// A fires it, B backs out, Y returns the highlight to the centre.
public enum RingNav: Sendable, Equatable {
    case up, down, left, right, confirm, back, centre
    /// The left stick's or a Siri Remote swipe's 60° sector: slot `k` clockwise from 12 o'clock,
    /// `nil` back at neutral.
    /// A dial follows the thumb — stepping one disc per push is the thing it must not do.
    case sector(Int?)
}
