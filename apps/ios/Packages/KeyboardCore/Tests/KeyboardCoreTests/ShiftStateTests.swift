import Testing
import KeyboardCore

@Test func singleTapCapitalisesOnce() {
    #expect(ShiftState.off.tapped(sinceLastTap: nil) == .once)
    #expect(ShiftState.once.afterCharacter == .off)
}

@Test func doubleTapLocks() {
    #expect(ShiftState.once.tapped(sinceLastTap: 0.2) == .locked)
    #expect(ShiftState.locked.afterCharacter == .locked)
}

@Test func slowSecondTapTurnsShiftOff() {
    #expect(ShiftState.once.tapped(sinceLastTap: 1.0) == .off)
}

@Test func tapWhileLockedTurnsShiftOff() {
    #expect(ShiftState.locked.tapped(sinceLastTap: 0.1) == .off)
}
