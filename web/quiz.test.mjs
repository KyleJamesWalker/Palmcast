import { test } from 'node:test';
import assert from 'node:assert/strict';

const { renderOptions, sendLabel } = await import('./quiz.js');

test('the label says what the button will do, and then that it did it', () => {
  assert.equal(sendLabel(0, false), 'Pick an answer');
  assert.equal(sendLabel(1, false), 'Send 1 answer');
  assert.equal(sendLabel(2, false), 'Send 2 answers');
  assert.equal(sendLabel(2, true), 'Sent');
  // Sent wins over the count, because the count is what was sent.
  assert.equal(sendLabel(0, true), 'Sent');
});

/// A minimal element stand-in, enough for the renderer's dom calls.
function stubDom() {
  globalThis.document = {
    createElement(tag) {
      return {
        tag,
        className: '',
        textContent: '',
        type: '',
        disabled: false,
        dataset: {},
        style: { setProperty() {} },
        children: [],
        classList: {
          names: [],
          add(...n) {
            this.names.push(...n);
          },
          contains(n) {
            return this.names.includes(n);
          },
        },
        setAttribute(k, v) {
          this.dataset[k] = v;
        },
        addEventListener() {},
        append(...kids) {
          this.children.push(...kids);
        },
      };
    },
  };
  return {
    innerHTML: '',
    hidden: false,
    children: [],
    append(...k) {
      this.children.push(...k);
    },
    querySelectorAll: () => [],
    contains: () => false,
  };
}

const MULTI = { multi: true, options: ['a', 'b', 'c'] };

function sendButton(root) {
  return root.children.find((c) => c.className?.includes('option-send'));
}

test('a pick-all question that was sent says so on a button that is done', () => {
  const root = stubDom();
  renderOptions(root, MULTI, { interactive: true, sent: true, chosen: [0, 1] });
  const send = sendButton(root);
  assert.ok(send, 'the send button was never drawn');
  assert.equal(send.textContent, 'Sent');
  assert.equal(send.disabled, true);
  assert.ok(send.classList.contains('sent'), 'nothing marked the button as sent');
  delete globalThis.document;
});

test('a selection not yet sent offers to send it', () => {
  const root = stubDom();
  renderOptions(root, MULTI, { interactive: true, sent: false, chosen: [0, 1] });
  const send = sendButton(root);
  assert.equal(send.textContent, 'Send 2 answers');
  assert.equal(send.disabled, false);
  delete globalThis.document;
});

test('the options stay tappable after a send, so a changed mind can replace it', () => {
  const root = stubDom();
  renderOptions(root, MULTI, { interactive: true, sent: true, chosen: [0] });
  const options = root.children.filter((c) => c.className === 'option');
  assert.equal(options.length, 3);
  for (const option of options) {
    assert.ok(!option.dataset['aria-disabled'], 'a sent answer stopped taking taps');
  }
  delete globalThis.document;
});

test('a revealed question is locked and offers no send at all', () => {
  const root = stubDom();
  renderOptions(root, MULTI, { interactive: true, locked: true, sent: true, chosen: [0] });
  assert.equal(sendButton(root), undefined, 'a locked question still offered a send');
  delete globalThis.document;
});
