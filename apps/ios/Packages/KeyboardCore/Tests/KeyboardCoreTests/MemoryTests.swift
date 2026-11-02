import Testing
import KeyboardCore

@Test func footprintIsReadable() {
    #expect(MemoryMonitor.footprintBytes() > 1_000_000)
}

@Test func budgetShedsAtEightyPercent() {
    let budget = MemoryBudget(limitBytes: 100)
    #expect(budget.pressure(footprintBytes: 79) == .normal)
    #expect(budget.pressure(footprintBytes: 80) == .shed)
    #expect(budget.pressure(footprintBytes: 150) == .shed)
}

@Test func keyboardExtensionBudgetIsFortyMegabytes() {
    #expect(MemoryBudget.keyboardExtension.limitBytes == 41_943_040)
}

@Test func gateTurnsOffOverBudget() {
    var gate = PredictionGate(budget: MemoryBudget(limitBytes: 100))
    #expect(gate.update(footprintBytes: 50, now: 0) == nil)
    #expect(gate.update(footprintBytes: 85, now: 1) == .off)
    #expect(!gate.isOn)
    #expect(gate.update(footprintBytes: 90, now: 2) == nil, "already off")
}

@Test func gateComesBackOnceMemoryDropsAndTheCooldownPasses() {
    var gate = PredictionGate(budget: MemoryBudget(limitBytes: 100))
    #expect(gate.update(footprintBytes: 85, now: 0) == .off)
    #expect(gate.update(footprintBytes: 40, now: 5) == nil, "too soon")
    #expect(gate.update(footprintBytes: 70, now: 20) == nil, "still too high")
    #expect(gate.update(footprintBytes: 59, now: 20) == .on)
    #expect(gate.isOn)
}

@Test func aMemoryWarningTurnsTheGateOffForTheCooldown() {
    var gate = PredictionGate(budget: MemoryBudget(limitBytes: 100))
    #expect(gate.memoryWarning(now: 0) == .off)
    #expect(gate.memoryWarning(now: 1) == nil)
    #expect(gate.update(footprintBytes: 10, now: 9) == nil)
    #expect(gate.update(footprintBytes: 10, now: 11) == .on, "a warning restarts the cooldown")
}
