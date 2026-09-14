import { test } from 'node:test';
import assert from 'node:assert/strict';

import { DECK_RULES, agentPrompt, fenceFor, starterPrompt } from './deckstate.js';
import { SAMPLE } from './sample.js';

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

test('the starter prompt carries the format rules and no deck', () => {
  const prompt = starterPrompt();
  assert.ok(prompt.includes(DECK_RULES), 'the rules are the point of this prompt');
  assert.ok(!prompt.includes('# '), 'nothing to quote: there is no deck yet');
  assert.match(prompt, /paste your topic, your notes, or an existing deck/);
});

test('both prompts state the rules the same way', () => {
  // One deck format. Two prompts that described it differently would send an
  // agent two ways of writing the same thing.
  assert.ok(starterPrompt().includes(DECK_RULES));
  assert.ok(agentPrompt('# Deck').includes(DECK_RULES));
});

test('the rules cover every mark the parser treats specially', () => {
  for (const mark of ['---', '???', '- [ ]', '- [x]', 'fenced code block']) {
    assert.ok(DECK_RULES.includes(mark), `the rules never mention ${mark}`);
  }
});

test('the fence outruns the longest backtick run in the deck', () => {
  assert.equal(fenceFor('no fences here'), '````');
  assert.equal(fenceFor('```md\n* one\n```'), '````');
  assert.equal(fenceFor('````\n```\n````'), '`````');
});

test('a deck holding a fence is quoted whole', () => {
  // The sample deck shows the format in a fenced block, so it is the deck the
  // start page hands an agent first. Three backticks would end the prompt
  // inside that example.
  const p = agentPrompt(SAMPLE);
  const fence = fenceFor(SAMPLE);
  assert.ok(p.includes(`${fence}markdown\n${SAMPLE.trimEnd()}\n${fence}`));
  assert.ok(p.includes('Now delete all of this'), 'the deck was cut short');
});

test('both prompts ask for a reply in a fence the deck cannot close', () => {
  for (const prompt of [starterPrompt(), agentPrompt(SAMPLE)]) {
    assert.match(prompt, /Wrap the whole deck in one block fenced with \d+ backticks/);
  }
});

test('the rules cover the marks that change how a slide arrives', () => {
  for (const mark of ['`*`', '`1)`', '`-`', '`1.`']) {
    assert.ok(DECK_RULES.includes(mark), `the rules never mention ${mark}`);
  }
});

test('the rules cover what a deck can and cannot draw', () => {
  assert.match(DECK_RULES, /!\[alt\]/, 'an agent cannot guess that images work');
  assert.match(DECK_RULES, /tables/);
  assert.match(DECK_RULES, /Raw HTML is shown as text/, 'agents reach for HTML unprompted');
});

test('a setext heading is called out, because it silently splits nothing', () => {
  assert.match(DECK_RULES, /setext/);
});
