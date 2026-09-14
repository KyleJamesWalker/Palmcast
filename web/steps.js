/// Shows as much of a slide as the room has been walked to.
///
/// The parser marks the staged items, so this only toggles what is already on
/// the page. A slide that stages nothing has no marked items and is untouched.
export function applySteps(root, step) {
  for (const item of root.querySelectorAll('[data-step]')) {
    item.hidden = Number(item.dataset.step) > step;
  }
}

/// Where the next press lands, or null at the end of the deck.
///
/// A slide is walked through before the deck moves on, which is what makes a
/// staged list a list that arrives one line at a time.
export function forward(slides, current, step) {
  if (step < steps(slides, current)) return { index: current, step: step + 1 };
  if (current < slides.length - 1) return { index: current + 1, step: 0 };
  return null;
}

/// Where a press back lands, or null at the start.
///
/// Stepping back onto an earlier slide shows it whole. The room has already
/// seen all of it, and walking a list backwards item by item helps nobody.
export function backward(slides, current, step) {
  if (step > 0) return { index: current, step: step - 1 };
  if (current > 0) return { index: current - 1, step: steps(slides, current - 1) };
  return null;
}

export function steps(slides, index) {
  return slides[index]?.steps ?? 0;
}
