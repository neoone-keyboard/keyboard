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
