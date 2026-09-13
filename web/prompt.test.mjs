import { test } from 'node:test';
import assert from 'node:assert/strict';

const { agentPrompt } = await import('./deckstate.js');

const DECK = '# Round one\n\n---\n\n# Round two';

test('the prompt carries the deck', () => {
  const p = agentPrompt(DECK, []);
  assert.ok(p.includes('# Round one'));
  assert.ok(p.includes('# Round two'));
});

test('the prompt teaches the format rules', () => {
  const p = agentPrompt(DECK, []);
  for (const rule of ['`---`', '`???`', '- [x]', 'fenced code block']) {
    assert.ok(p.includes(rule), `missing ${rule}`);
  }
});

test('open questions come along, with their votes', () => {
  const p = agentPrompt(DECK, [
    { id: 1, text: 'Why not Go?', votes: 3, answered: false },
    { id: 2, text: 'One vote only', votes: 1, answered: false },
  ]);
  assert.ok(p.includes('(3 votes) Why not Go?'));
  assert.ok(p.includes('(1 vote) One vote only'), 'singular vote not handled');
});

test('an answered question is not re-asked', () => {
  const p = agentPrompt(DECK, [{ id: 1, text: 'Already covered', votes: 9, answered: true }]);
  assert.ok(!p.includes('Already covered'));
  assert.ok(p.includes('none yet'));
});

test('no questions reads sensibly', () => {
  assert.ok(agentPrompt(DECK).includes('- none yet'));
});

test('the deck is fenced so an agent can tell it from the instructions', () => {
  const p = agentPrompt(DECK, []);
  assert.ok(p.includes('```markdown'));
});
