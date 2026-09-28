// Benchmark macro: multiplies the selected column by 1.1.
for (var r = ROWMIN; r <= ROWMAX; ++r) {
    setCell(r, COLMIN, getFloat(r, COLMIN) * 1.1);
}
