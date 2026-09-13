/// Keeps the reader's place across a rebuild.
///
/// These lists are redrawn whole whenever the server sends an update, which in
/// a busy room is constantly. Wiping the container takes focus with it, so a
/// keyboard reader is thrown back to the top of the page by other people's
/// activity. Both helpers identify a control by the attribute its owner row
/// carries plus the control's own class.
export function holdFocus(root, attribute) {
  const active = document.activeElement;
  if (!active || !root.contains(active)) return null;
  const owner = active.closest(`[${attribute}]`);
  if (!owner) return null;
  return { value: owner.getAttribute(attribute), control: active.classList[0] };
}

export function returnFocus(root, attribute, held) {
  if (!held || held.value == null) return;
  const owner = [...root.querySelectorAll(`[${attribute}]`)].find(
    (node) => node.getAttribute(attribute) === held.value,
  );
  if (!owner) return;

  const target = owner.classList.contains(held.control)
    ? owner
    : owner.querySelector(`.${held.control}`);
  if (target && !target.disabled) {
    target.focus();
    return;
  }
  // The control went away, so land somewhere in the same row rather than at the
  // top of the page.
  if (!owner.disabled && typeof owner.focus === 'function' && owner.tabIndex >= 0) {
    owner.focus();
    return;
  }
  owner.querySelector?.('button:not([disabled])')?.focus();
}
