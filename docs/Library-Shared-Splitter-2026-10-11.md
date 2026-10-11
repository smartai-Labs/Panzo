# Asset library shared divider and scrollbar fix (2026-10-11)

Scope: asset library only. The shared divider component and other pages were not changed.

- The library inspector now uses `pane_splitter::Splitter` and its `Drag` state for painting, cursor, capture, and size calculation. Its interaction band is 6 DIP, matching the video workbench. Pointer coordinates remain relative to the parent while the child divider moves.
- Escape, cancellation, or loss of capture restores the previous inspector width, including automatic sizing. Successful release retains the resized width.
- Removed `WS_VSCROLL` from the initial LISTBOX creation. Stripping that flag after creation had allowed native scrolling to revive the scrollbar during `LB_SETTOPINDEX`, paint a white rectangle, and hide it again before the final buffered repaint.
- Added native intermediate-message regression coverage for both grid and list views. It checks native scrollbar state, non-client painting, and white pixels before the final repaint can hide the issue.

Validation:
- Shared splitter unit tests and scaled library layout test passed.
- Native 145-asset test passed: 40 drag updates plus 40 wheel messages per view, zero revived native scrollbars, zero non-client paints, zero white-pixel samples. Selection and lazy thumbnail loading remained intact.
- Native visual acceptance passed for dark/light themes, preview resizing, card reflow, zoom, information popup and preview popup. Across 192 panel drag steps, all 36 sampled frames matched a clean repaint (zero differing pixels).
- The reflow fixture now uses DIP sizes and a card whose column actually changes, then restores the original viewport for later scrolling checks.

Evidence: `.tmp/library-shared-splitter/verified/scroll-and-splitter.txt`, `.tmp/library-shared-splitter/accepted/`, and `.tmp/library-shared-splitter/accepted-test.log`.
Build: `target/desktop-v14-library-splitter-fix/release/`.
