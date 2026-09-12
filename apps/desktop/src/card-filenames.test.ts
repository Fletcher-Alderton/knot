import { describe, expect, it } from 'vitest';
import { shareCardPath } from './main';
import { findNote } from './markdown';
const id='01ARZ3NDEKTSV4RRFFQ69G0001';
describe('card filenames',()=>{
  it('uses column ID, safe title and unchanged ID',()=>{
    expect(shareCardPath('/board',id,{column:'group-project',title:'Sprint 4: review'})).toBe(`/board/cards/group-project-Sprint-4-review-${id}.md`);
    expect(shareCardPath('/board',id,{column:'todo',title:'///'})).toBe(`/board/cards/todo-card-${id}.md`);
    expect(()=>shareCardPath('/board','../bad')).toThrow();
  });
  it('resolves old composite links after title and column change',()=>{
    const note={id,title:'Renamed',body:'Body'};
    expect(findNote(`cards/todo-Old-title-${id}.md#Heading`,{notes:[note]})).toBe(note);
    expect(findNote(`${id}.md`,{notes:[note]})).toBe(note);
  });
});
