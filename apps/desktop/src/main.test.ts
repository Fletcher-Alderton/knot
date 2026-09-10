import { describe, expect, it, beforeEach, vi } from 'vitest';
import { renderCardText } from './testables';
import { state, normalize, applyBoardInfo, loadCards, pairAddress, syncNow, openBoard, moveCard, renameBoard, renameColumn, createColumn, createLabel, updateLabel, deleteLabel, reorderColumns, setView, render, applyDownloadProgress, setInvokeForTests, setDirectoryPickerForTests, loadModels, loadOpenAIModels, checkOpenAIAccess, renderMarkdown, sharePath, shareCardPath } from './main';

describe('Kanban UI',()=>{
 it('renders Markdown safely and preserves only safe links',()=>{const html=renderMarkdown('[docs](https://example.com) [bad](javascript:alert(1)) <script>alert(1)</script>');expect(html).toContain('href="https://example.com"');expect(html).not.toContain('javascript:');expect(html).not.toContain('<script>');});
 it('renders Obsidian task syntax as compact task elements',()=>{const html=renderMarkdown('- [x] Done\n- [ ] Todo');expect(html).toContain('class="task-checkbox is-checked"');expect(html).toContain('class="task-checkbox"');expect(html).not.toContain('<input');});
 it('shares POSIX root-relative card paths',()=>{expect(sharePath('\\Users\\me\\board\\card.md')).toBe('/Users/me/board/card.md');expect(sharePath('/Users/me/board/card.md','/Users/me')).toBe('/board/card.md');expect(shareCardPath('/Users/me/board','abc')).toBe('/cards/abc.md');});
 it('escapes card content before rendering',()=>expect(renderCardText('<script>')).toBe('&lt;script&gt;'));
 it('normalizes backend card payloads',()=>expect(normalize({id:'a',title:'A',body:'B',column:'doing',labels:['x'],updated_at:'2024-01-01T00:00:00Z'})).toMatchObject({id:'a',column:'doing',labels:['x'],updatedAt:1704067200000}));
 it('keeps conflict choices explicit',()=>expect(['local','remote','manual']).toHaveLength(3));
 it('hydrates cards from list_cards and surfaces invoke failures',async()=>{
   state.boardPath='/board'; state.cards=[];
   const invoke=vi.fn(async (command:string)=>{if(command==='list_cards')return [{id:'real',title:'From disk',body:'body',column:'done',labels:[]}]; throw new Error('offline');});
   setInvokeForTests(invoke as any); await loadCards();
   expect(state.cards[0].id).toBe('real'); expect(invoke).toHaveBeenCalledWith('list_cards',{path:'/board'});
   setInvokeForTests(async()=>{throw new Error('disk unavailable')}); await loadCards();
   expect(state.error).toContain('disk unavailable'); setInvokeForTests(null);
 });
 it('hydrates the stable board identity without using its path as identity',()=>{applyBoardInfo({path:'/private/local/path',board_id:'01BOARDULID',cards:[]});expect(state.boardId).toBe('01BOARDULID');expect(state.boardId).not.toBe(state.boardPath)});
 it('hydrates board labels and exposes color CRUD in Settings',async()=>{state.boardPath='/board';state.view='settings';applyBoardInfo({path:'/board',board_id:'labels-board',title:'Board',columns:[],labels:{urgent:'#ef4444'},cards:[]});expect(state.labelColors.urgent).toBe('#ef4444');render();expect(document.querySelector('[data-settings-section=labels]')).not.toBeNull();const info={path:'/board',board_id:'labels-board',title:'Board',columns:[],labels:{urgent:'#ef4444',later:'#22c55e'},cards:[]};const invoke=vi.fn(async(command:string)=>command==='create_label'?info:command==='update_label'?{...info,labels:{soon:'#3b82f6'}}:{...info,labels:{}});setInvokeForTests(invoke as any);document.querySelector<HTMLInputElement>('#new-label-name')!.value='later';document.querySelector<HTMLFormElement>('#create-label-form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('create_label',{path:'/board',name:'later',color:'#8b5cf6'}));await updateLabel('urgent','soon','#3b82f6');await deleteLabel('soon');expect(invoke).toHaveBeenCalledWith('update_label',{path:'/board',name:'urgent',newName:'soon',color:'#3b82f6'});expect(invoke).toHaveBeenCalledWith('delete_label',{path:'/board',name:'soon'});setInvokeForTests(null);});
it('selects board labels with a multi-select and preserves them in the save payload',async()=>{
   state.boardPath='/board';state.view='board';state.labels={urgent:'#ef4444',later:'#22c55e'};state.labelColors={...state.labels};state.columns=[{id:'todo',name:'Todo'}];state.cards=[{id:'card-1',title:'Card',body:'',column:'todo',labels:['urgent'],updatedAt:0}];state.selected='card-1';
   const invoke=vi.fn(async(command:string)=>{if(command==='update_card')return {id:'card-1',title:'Card',body:'',column:'todo',labels:['urgent','later']};throw new Error(command)});setInvokeForTests(invoke as any);render();
   const select=document.querySelector<HTMLSelectElement>('#labels')!;expect(select.multiple).toBe(true);expect(Array.from(select.options).map(option=>option.value)).toEqual(['urgent','later']);expect(select.options[0].selected).toBe(true);select.options[1].selected=true;document.querySelector<HTMLElement>('#save')!.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('update_card',{path:'/board',id:'card-1',input:expect.objectContaining({labels:['urgent','later'],label_colors:{urgent:'#ef4444',later:'#22c55e'}})}));setInvokeForTests(null);
 });
it('pairs a serialized endpoint address for the current board',async()=>{
   state.boardPath='/board';state.boardId='01BOARDULID'; const address=JSON.stringify({id:'peer-1',addrs:['relay']});
   const invoke=vi.fn(async(command:string)=>{if(command==='pair_peer_address')return {peer_id:'peer-1',trusted:true};if(command==='sync_status')return {connected:true,trusted_peers:1,connection:'connected'};if(command==='endpoint_info')return {endpoint_id:'local',address:'local-address'};if(command==='list_trusted_peers')return [{peer_id:'peer-1',trusted:true}];throw new Error(command)});
   setInvokeForTests(invoke as any); await pairAddress(address);
   expect(invoke).toHaveBeenCalledWith('pair_peer_address',{peerId:'peer-1',address,authorizedBoards:['01BOARDULID']});
   expect(state.peerAddress).toBe(address);
 });
 it('syncs the board then refreshes cards and status',async()=>{
   state.boardPath='/board';state.boardId='01BOARDULID';state.peerId='peer-1';state.peerAddress='serialized';
   const invoke=vi.fn(async(command:string)=>{if(command==='sync_board')return {status:'connected',transferred:1,received:1};if(command==='list_cards')return [{id:'synced',title:'Synced',body:'',column:'backlog',labels:[]}];if(command==='sync_status')return {connected:true,trusted_peers:1,connection:'connected'};if(command==='endpoint_info')return {endpoint_id:'local',address:'local-address'};if(command==='list_trusted_peers')return [{peer_id:'peer-1',trusted:true}];throw new Error(command)});
   setInvokeForTests(invoke as any);await syncNow();
   expect(invoke).toHaveBeenCalledWith('sync_board',{path:'/board',peerId:'peer-1',address:'serialized'});expect(state.cards[0].id).toBe('synced');expect(state.error).toBe('');expect(state.connection).toBe('connected');
   setInvokeForTests(null);
 });
 it('smart-opens an existing board or initializes the selected directory',async()=>{
   setDirectoryPickerForTests(async()=>'/chosen-board');
   const invoke=vi.fn(async(command:string)=>{if(command==='open_or_create_board')return {path:'/chosen-board',board_id:'01BOARDULID',title:'Chosen board',columns:[{id:'backlog',name:'Backlog'}],cards:[]};if(command==='watch_board')return true;if(command==='list_cards')return [];if(command==='sync_status')return {connected:false,trusted_peers:0,connection:'offline'};if(command==='endpoint_info')return {endpoint_id:'local',address:'local-address'};if(command==='list_trusted_peers')return [];throw new Error(command)});
   setInvokeForTests(invoke as any);await openBoard();
   expect(invoke).toHaveBeenCalledWith('open_or_create_board',{path:'/chosen-board'});
   expect(state.boardPath).toBe('/chosen-board');expect(state.boardId).toBe('01BOARDULID');expect(state.boardTitle).toBe('Chosen board');expect(state.error).toBe('');
   setDirectoryPickerForTests(null);setInvokeForTests(null);
 });
 it('shows one icon-only smart board action',()=>{
   state.boardPath='/board';state.view='board';render();
   expect(document.querySelector('#open')?.textContent?.trim()).toBe('＋');expect(document.querySelector('#create')).toBeNull();
 });
 it('keeps the board header outside the scrollable columns viewport',()=>{
   state.boardPath='/board';state.view='board';state.columns=[{id:'todo',name:'Todo'}];state.cards=[];render();
   expect(document.querySelector('.toolbar')?.parentElement?.querySelector('.board-scroll')).not.toBeNull();
   expect(document.querySelector('.board-scroll')?.querySelector('.columns')).not.toBeNull();
 });
 it('removes redundant board and card chrome while keeping accessible compact actions',()=>{
   state.boardPath='/board';state.boardTitle='Board';state.view='board';state.columns=[{id:'todo',name:'Todo'}];state.cards=Array.from({length:7},(_,i)=>({id:`card-${i}`,title:`Card ${i}`,body:'',column:'todo',labels:[],updatedAt:0}));state.selected=null;render();
   const page=document.querySelector('[data-page=board]')!;
   expect(page.textContent).not.toContain('changes saved to Markdown');expect(page.textContent).not.toContain('Drop cards here');
   expect(document.querySelector('.drag')).toBeNull();expect(document.querySelector('[data-delete]')).toBeNull();expect(document.querySelector('.drop-add')).toBeNull();
   const createActions=document.querySelector('.create-actions');expect(createActions?.getAttribute('role')).toBe('group');
   expect(Array.from(createActions!.children).map(x=>x.id)).toEqual(['new-card','add-column']);expect(document.querySelector('#new-card')?.parentElement).toBe(document.querySelector('#add-column')?.parentElement);
   expect(document.querySelector('#new-card')?.textContent).toContain('Add card');expect(document.querySelector('#new-card')?.classList.contains('card-action')).toBe(true);
   expect(document.querySelector('#add-column')?.textContent).toContain('Add column');expect(document.querySelector('#add-column')?.classList.contains('column-action')).toBe(true);
   expect(document.querySelector('[data-add]')?.getAttribute('aria-label')).toContain('Add card');
 });
 it('persists card drops at the requested position',async()=>{
   state.boardPath='/board';state.cards=[{id:'a',title:'A',body:'',column:'todo',labels:[],updatedAt:0},{id:'b',title:'B',body:'',column:'todo',labels:[],updatedAt:0}];const invoke=vi.fn(async()=>({id:'a',title:'A',body:'',column:'todo',position:1500,labels:[]}));setInvokeForTests(invoke as any);
   await moveCard('a','todo',1500);expect(invoke).toHaveBeenCalledWith('move_card',{path:'/board',id:'a',column:'todo',position:1500});setInvokeForTests(null);
 });
 it('moves a card between columns at the dropped insertion point',async()=>{
   state.boardPath='/board';state.columns=[{id:'todo',name:'Todo'},{id:'done',name:'Done'}];state.cards=[{id:'a',title:'A',body:'',column:'todo',labels:[],updatedAt:0},{id:'b',title:'B',body:'',column:'done',labels:[],updatedAt:0,position:1000},{id:'c',title:'C',body:'',column:'done',labels:[],updatedAt:0,position:2000}];const invoke=vi.fn(async()=>({id:'a',title:'A',body:'',column:'done',position:1500,labels:[]}));setInvokeForTests(invoke as any);render();const target=document.querySelector<HTMLElement>('[data-id=c]')!;Object.defineProperty(target,'getBoundingClientRect',{value:()=>({top:100,height:40,bottom:140,left:0,right:100,width:100}),configurable:true});const source=document.querySelector<HTMLElement>('[data-id=a]')!;Object.defineProperty(document,'elementFromPoint',{value:vi.fn(()=>target),configurable:true});source.dispatchEvent(new MouseEvent('pointerdown',{bubbles:true,clientX:10,clientY:10,button:0}));document.dispatchEvent(new MouseEvent('pointermove',{bubbles:true,clientX:10,clientY:110,buttons:1}));document.dispatchEvent(new MouseEvent('pointerup',{bubbles:true,clientX:10,clientY:110,button:0}));await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('move_card',{path:'/board',id:'a',column:'done',position:1500}));setInvokeForTests(null);
 });
 it('moves a card across dynamic columns and persists the drop',async()=>{
   state.boardPath='/board';state.columns=[{id:'todo',name:'Todo'},{id:'review',name:'Review'}];state.cards=[{id:'card-1',title:'Card',body:'',column:'todo',labels:[],updatedAt:0}];
   const invoke=vi.fn(async()=>({id:'card-1',title:'Card',body:'',column:'review',labels:[]}));setInvokeForTests(invoke as any);
   await moveCard('card-1','review');
   expect(invoke).toHaveBeenCalledWith('move_card',{path:'/board',id:'card-1',column:'review',position:1000});
   expect(state.cards[0].column).toBe('review');setInvokeForTests(null);
 });
 it('wires browser drag events to the destination dropzone',async()=>{
   state.boardPath='/board';state.boardTitle='Board';state.view='board';state.columns=[{id:'todo',name:'Todo'},{id:'done',name:'Done'}];state.cards=[{id:'card-dnd',title:'Drag me',body:'',column:'todo',labels:[],updatedAt:0}];state.selected=null;state.error='';
   const invoke=vi.fn(async()=>({id:'card-dnd',title:'Drag me',body:'',column:'done',labels:[]}));setInvokeForTests(invoke as any);render();
   const values=new Map<string,string>();const dataTransfer={effectAllowed:'none',dropEffect:'none',setData:(type:string,value:string)=>values.set(type,value),getData:(type:string)=>values.get(type)||''};
   const dragStart=new Event('dragstart',{bubbles:true,cancelable:true});Object.defineProperty(dragStart,'dataTransfer',{value:dataTransfer});document.querySelector<HTMLElement>('[data-id=card-dnd]')!.dispatchEvent(dragStart);
   const drop=new Event('drop',{bubbles:true,cancelable:true});Object.defineProperty(drop,'dataTransfer',{value:dataTransfer});document.querySelector<HTMLElement>('[data-drop-column=done]')!.dispatchEvent(drop);
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('move_card',{path:'/board',id:'card-dnd',column:'done',position:1000}));
   expect(state.cards[0].column).toBe('done');setInvokeForTests(null);
 });
 it('renames boards and columns and creates columns through persisted commands',async()=>{
   state.boardPath='/board';state.boardId='board-id';state.boardTitle='Old';state.columns=[{id:'backlog',name:'Backlog'}];
   const info={path:'/board',board_id:'board-id',title:'Roadmap',columns:[{id:'backlog',name:'Ideas'},{id:'qa',name:'QA'}],cards:[]};
   const invoke=vi.fn(async(command:string)=>command==='rename_board'?{...info,columns:[{id:'backlog',name:'Backlog'}]}:command==='rename_column'?{...info,columns:[{id:'backlog',name:'Ideas'}]}:info);setInvokeForTests(invoke as any);
   await renameBoard('Roadmap');await renameColumn('backlog','Ideas');await createColumn('QA');
   expect(invoke).toHaveBeenCalledWith('rename_board',{path:'/board',title:'Roadmap'});
   expect(invoke).toHaveBeenCalledWith('rename_column',{path:'/board',columnId:'backlog',name:'Ideas'});
   expect(invoke).toHaveBeenCalledWith('create_column',{path:'/board',name:'QA'});
   expect(state.columns.some(column=>column.id==='qa')).toBe(true);setInvokeForTests(null);
 });
 it('persists reordered columns',async()=>{
   state.boardPath='/board';state.columns=[{id:'backlog',name:'Backlog'},{id:'doing',name:'Doing'},{id:'done',name:'Done'}];
   const invoke=vi.fn(async()=>({path:'/board',board_id:'board-id',title:'Board',columns:[{id:'done',name:'Done'},{id:'backlog',name:'Backlog'},{id:'doing',name:'Doing'}],cards:[]}));setInvokeForTests(invoke as any);
   await reorderColumns(['done','backlog','doing']);
   expect(invoke).toHaveBeenCalledWith('reorder_columns',{path:'/board',columnIds:['done','backlog','doing']});expect(state.columns.map(c=>c.id)).toEqual(['done','backlog','doing']);setInvokeForTests(null);
 });
 it('reveals board switching from the title instead of rendering tabs',()=>{
   state.boardPath='/board';state.boardTitle='Roadmap';state.columns=[{id:'backlog',name:'Backlog'}];state.openBoards=[{path:'/board',boardId:'board-id',title:'Roadmap'},{path:'/notes',boardId:'notes-id',title:'Notes'}];state.cards=[];state.connected=true;state.error='';
   setView('board');render();
   expect(document.querySelector('[role=tablist]')).toBeNull();
   const titleCard=document.querySelector('.board-title-menu')!;expect(titleCard.querySelector('#rename-board')?.textContent).toBe('Roadmap');
   expect(titleCard.querySelector('[role=menu]')?.textContent).toContain('Notes');expect((titleCard.textContent?.match(/Roadmap/g)||[]).length).toBe(1);
   expect(document.querySelector('header')).toBeNull();
   expect(document.querySelector('.board-sync-status')?.textContent).toContain('Synced');
   expect(document.querySelector('.board-switcher #open')).not.toBeNull();
   expect(document.querySelector('.sidebar-sync')).toBeNull();
   setView('settings');render();
   expect(document.querySelector('[data-page=settings]')?.textContent).toContain('Settings');
   expect(document.querySelector('#pair')).not.toBeNull();expect(document.querySelector('#sync')).not.toBeNull();
 });
 it('uses the settings control to return to the board',()=>{
   state.boardPath='/board';state.view='board';render();
   document.querySelector<HTMLElement>('.settings-toggle')!.click();
   expect(state.view).toBe('settings');
   expect(document.querySelector('.settings-toggle')?.getAttribute('aria-label')).toBe('Return to board');
   document.querySelector<HTMLElement>('.settings-toggle')!.click();
   expect(state.view).toBe('board');
 });
 it('renames board and columns inline while creating columns in-app',async()=>{
   state.boardPath='/board';state.boardId='board-id';state.boardTitle='Old';state.view='board';state.columns=[{id:'backlog',name:'Backlog'}];state.cards=[];
   const info={path:'/board',board_id:'board-id',title:'Roadmap',columns:[{id:'backlog',name:'Backlog'}],cards:[]};
   const invoke=vi.fn(async()=>info);setInvokeForTests(invoke as any);render();
   document.querySelector<HTMLElement>('#rename-board')!.click();
   expect(document.querySelector('[role=dialog]')).toBeNull();
   const input=document.querySelector<HTMLInputElement>('#inline-title-input')!;input.value='Roadmap';
   input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('rename_board',{path:'/board',title:'Roadmap'}));
   document.querySelector<HTMLElement>('#add-column')!.click();
   expect(document.querySelector('[role=dialog]')?.textContent).toContain('Add column');
   document.querySelector<HTMLInputElement>('#dialog-input')!.value='QA';
   document.querySelector<HTMLFormElement>('#input-dialog-form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('create_column',{path:'/board',name:'QA'}));
   document.querySelector<HTMLElement>('[data-rename-column=backlog]')!.click();
   expect(document.querySelector('[role=dialog]')).toBeNull();
   expect((document.querySelector<HTMLInputElement>('#inline-column-input')!).value).toBe('Backlog');
   document.querySelector<HTMLInputElement>('#inline-column-input')!.value='Ideas';
   document.querySelector<HTMLInputElement>('#inline-column-input')!.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('rename_column',{path:'/board',columnId:'backlog',name:'Ideas'}));
   setInvokeForTests(null);
 });
 it('asks for confirmation in-app before deleting a card',async()=>{
   state.boardPath='/board';state.view='board';state.columns=[{id:'backlog',name:'Backlog'}];state.cards=[{id:'delete-me',title:'Delete me',body:'',column:'backlog',labels:[],updatedAt:0}];state.selected='delete-me';
   const invoke=vi.fn(async()=>true);setInvokeForTests(invoke as any);render();
   document.querySelector<HTMLElement>('#delete')!.click();
   expect(document.querySelector('[role=dialog]')?.textContent).toContain('Delete card?');
   expect(invoke).not.toHaveBeenCalled();
   document.querySelector<HTMLElement>('#dialog-cancel')!.click();
   expect(state.cards.some(card=>card.id==='delete-me')).toBe(true);expect(state.selected).toBe('delete-me');
   document.querySelector<HTMLElement>('#delete')!.click();
   document.querySelector<HTMLElement>('#dialog-confirm')!.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('delete_card',{path:'/board',id:'delete-me'}));
   setInvokeForTests(null);
 });
 it('collects and submits peer addresses in an in-app dialog',async()=>{
   state.boardPath='/board';state.boardId='board-id';state.view='settings';state.selected=null;state.dialog=null;
   const address=JSON.stringify({id:'peer-dialog',addrs:['relay']});
   const invoke=vi.fn(async(command:string)=>{if(command==='pair_peer_address')return {peer_id:'peer-dialog',trusted:true};if(command==='sync_status')return {connected:true,trusted_peers:1,connection:'connected'};if(command==='endpoint_info')return {endpoint_id:'local',address:'local-address'};if(command==='list_trusted_peers')return [{peer_id:'peer-dialog',trusted:true,address}];throw new Error(command)});setInvokeForTests(invoke as any);render();
   document.querySelector<HTMLElement>('#pair')!.click();
   expect(document.querySelector('[role=dialog]')?.textContent).toContain('Pair device');
   expect(document.querySelector('#dialog-input')?.tagName).toBe('TEXTAREA');
   document.querySelector<HTMLTextAreaElement>('#dialog-input')!.value=address;
   document.querySelector<HTMLFormElement>('#input-dialog-form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('pair_peer_address',{peerId:'peer-dialog',address,authorizedBoards:['board-id']}));
   setInvokeForTests(null);
 });
 it('reorders columns by dragging their headers',async()=>{
   state.boardPath='/board';state.boardId='board-id';state.boardTitle='Board';state.view='board';state.columns=[{id:'todo',name:'Todo'},{id:'done',name:'Done'}];state.cards=[];
   const info={path:'/board',board_id:'board-id',title:'Board',columns:[{id:'done',name:'Done'},{id:'todo',name:'Todo'}],cards:[]};const invoke=vi.fn(async()=>info);setInvokeForTests(invoke as any);render();
   const source=document.querySelector<HTMLElement>('[data-column=todo]')!,handle=source.querySelector<HTMLElement>('.column-head')!,target=document.querySelector<HTMLElement>('[data-column=done]')!;
   Object.defineProperty(target,'getBoundingClientRect',{value:()=>({left:200,width:100,top:0,right:300,bottom:300,height:300,x:200,y:0,toJSON(){}}),configurable:true});Object.defineProperty(document,'elementFromPoint',{value:vi.fn(()=>target),configurable:true});
   handle.dispatchEvent(new MouseEvent('pointerdown',{bubbles:true,clientX:10,clientY:10,button:0}));document.dispatchEvent(new MouseEvent('pointermove',{bubbles:true,clientX:275,clientY:20,buttons:1}));
   const columnGhost=document.querySelector<HTMLElement>('.column-drag-ghost');expect(columnGhost).not.toBeNull();expect(columnGhost!.style.transformOrigin).toBe('10px 10px');document.dispatchEvent(new MouseEvent('pointerup',{bubbles:true,clientX:275,clientY:20,button:0}));
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('reorder_columns',{path:'/board',columnIds:['done','todo']}));setInvokeForTests(null);
 });
 it('moves cards with pointer dragging when native HTML drag events are unavailable',async()=>{
   state.boardPath='/board';state.view='board';state.columns=[{id:'todo',name:'Todo'},{id:'done',name:'Done'}];state.cards=[{id:'pointer-card',title:'Pointer card',body:'',column:'todo',labels:[],updatedAt:0}];state.selected=null;
   const invoke=vi.fn(async()=>({id:'pointer-card',title:'Pointer card',body:'',column:'done',labels:[]}));setInvokeForTests(invoke as any);render();
   const card=document.querySelector<HTMLElement>('[data-id=pointer-card]')!;const target=document.querySelector<HTMLElement>('[data-drop-column=done]')!;
   const hit=vi.fn(()=>target);Object.defineProperty(document,'elementFromPoint',{value:hit,configurable:true});
   card.dispatchEvent(new MouseEvent('pointerdown',{bubbles:true,clientX:10,clientY:10,button:0}));
   document.dispatchEvent(new MouseEvent('pointermove',{bubbles:true,clientX:30,clientY:30,buttons:1}));
   expect(target.classList.contains('drag-over')).toBe(true);expect(card.classList.contains('dragging')).toBe(true);expect(document.querySelector('.card-drop-placeholder')).not.toBeNull();
   const ghost=document.querySelector<HTMLElement>('.drag-ghost');expect(ghost).not.toBeNull();expect(ghost!.style.transformOrigin).toBe('10px 10px');
   document.dispatchEvent(new MouseEvent('pointerup',{bubbles:true,clientX:30,clientY:30,button:0}));
   expect(document.querySelector('.drag-ghost')).toBeNull();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('move_card',{path:'/board',id:'pointer-card',column:'done',position:1000}));
   setInvokeForTests(null);
 });

 it('presents settings as a simple models-first workspace',()=>{
   state.view='settings';state.error='';state.localModels=[];state.hfSearchResults=[];state.modelSettings={};state.modelLoading=false;render();
   const page=document.querySelector('[data-page=settings]')!;
   expect(page.querySelector('h1')?.textContent).toBe('Settings');
   expect(page.querySelector('[data-settings-section=models]')).not.toBeNull();
   expect(page.querySelector('[data-settings-section=sync]')).not.toBeNull();
   expect(page.querySelector('.model-setup-status')?.textContent).toContain('Choose a model');
   expect(page.querySelectorAll('[data-install-recommended]').length).toBeGreaterThan(0);
   expect(page.textContent).not.toContain('HF GGUF filename');
 });

 it('installs and activates a recommended model in one click',async()=>{
   state.view='settings';state.error='';state.localModels=[];state.modelSettings={};state.modelLoading=false;
   const installed={id:'qwen.gguf',provider:'huggingface',name:'Qwen',source:'unsloth/Qwen3.5-4B-GGUF',size_bytes:2_600_000_000};
   const invoke=vi.fn(async(command:string)=>{if(command==='download_huggingface_gguf')return installed;if(command==='save_model_settings')return null;if(command==='model_settings')return {provider:'huggingface',model_id:'qwen.gguf'};if(command==='list_local_models')return [installed];throw new Error(command)});
   setInvokeForTests(invoke as any);render();document.querySelector<HTMLElement>('[data-install-recommended][data-repo="'+installed.source+'"]')!.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('download_huggingface_gguf',expect.objectContaining({repoId:'unsloth/Qwen3.5-4B-GGUF'})));
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('save_model_settings',{settings:{provider:'huggingface',model_id:'qwen.gguf'}}));
   expect(state.modelSettings.model_id).toBe('qwen.gguf');setInvokeForTests(null);
 });

 it('activates an installed model with one click',async()=>{
   state.view='settings';state.error='';state.localModels=[{id:'local.gguf',provider:'huggingface',name:'Local model',source:'repo',size_bytes:2048}];state.modelSettings={};render();
   const invoke=vi.fn(async(command:string)=>{if(command==='save_model_settings')return null;if(command==='model_settings')return {provider:'huggingface',model_id:'local.gguf'};if(command==='list_local_models')return state.localModels;throw new Error(command)});
   setInvokeForTests(invoke as any);render();document.querySelector<HTMLElement>('[data-select-model="local.gguf"]')!.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('save_model_settings',{settings:{provider:'huggingface',model_id:'local.gguf'}}));
   setInvokeForTests(null);
 });

 it('loads and unloads the selected model from the settings toggle',async()=>{
   state.view='settings';state.error='';state.modelLoading=false;state.modelFeedback='';state.localModels=[{id:'local.gguf',provider:'huggingface',name:'Local model',source:'repo',size_bytes:2048}];state.modelSettings={provider:'huggingface',model_id:'local.gguf',keep_model_loaded:false};
   const invoke=vi.fn(async(command:string)=>command==='unload_local_model'?true:null);setInvokeForTests(invoke as any);render();
   const toggle=document.querySelector<HTMLInputElement>('#keep-model-loaded')!;toggle.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('load_local_model',{id:'local.gguf'}));
   expect(invoke).toHaveBeenCalledWith('save_model_settings',{settings:expect.objectContaining({keep_model_loaded:true})});
   await vi.waitFor(()=>expect(state.modelSettings.keep_model_loaded).toBe(true));
   document.querySelector<HTMLInputElement>('#keep-model-loaded')!.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('unload_local_model',undefined));
   expect(state.modelSettings.keep_model_loaded).toBe(false);setInvokeForTests(null);
 });

 it('keeps the Ollama empty state accessible and refreshes the edited URL',async()=>{
   state.view='settings';state.error='';state.modelSettings={provider:'ollama',ollama_url:'http://127.0.0.1:11434'};state.ollamaModels=[];
   const invoke=vi.fn(async()=>[]);setInvokeForTests(invoke as any);render();
   expect(document.querySelectorAll('#refresh-ollama')).toHaveLength(1);
   document.querySelector<HTMLInputElement>('#ollama-url')!.value='http://localhost:11434';
   document.querySelector<HTMLElement>('#refresh-ollama')!.click();
   await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('list_ollama_models',{url:'http://localhost:11434'}));setInvokeForTests(null);
 });

 it('lets a search result fill the repository without copy and paste',()=>{
   state.view='settings';state.modelSettings={provider:'huggingface'};state.hfSearchResults=[{id:'owner/useful-GGUF',downloads:100,likes:4}];render();
   document.querySelector<HTMLElement>('[data-use-hf="owner/useful-GGUF"]')!.click();
   expect(document.querySelector<HTMLInputElement>('#hf-repo')?.value).toBe('owner/useful-GGUF');
 });

 it('shows determinate model download progress and live speed',()=>{
   state.view='settings';state.modelSettings={provider:'huggingface'};state.modelLoading=true;
   applyDownloadProgress({filename:'model.gguf',downloaded_bytes:536870912,total_bytes:1073741824,bytes_per_second:12582912});
   const progress=document.querySelector<HTMLProgressElement>('.download-progress progress')!;
   expect(progress.value).toBe(50);expect(progress.max).toBe(100);
   expect(document.querySelector('.download-progress')?.textContent).toContain('512.0 MB of 1.0 GB');
   expect(document.querySelector('.download-progress')?.textContent).toContain('12.0 MB/s');
 });

 it('runs only one Quick Add request while submission is pending',async()=>{
   state.boardPath='/board';state.view='board';state.cards=[];state.quickAddOpen=true;state.modelSettings={provider:'huggingface',model_id:'local.gguf'};
   let finishParse!:(value:any)=>void;
   const parsed=new Promise(resolve=>{finishParse=resolve;});
   const invoke=vi.fn(async(command:string)=>{if(command==='parse_quick_add')return parsed;if(command==='add_card')return {id:'new',title:'Task',body:'',column:'backlog',labels:[]};throw new Error(command)});
   setInvokeForTests(invoke as any);render();
   document.querySelector<HTMLInputElement>('#quick-add-input')!.value='Task tomorrow';
   const form=document.querySelector<HTMLFormElement>('#quick-add-form')!;
   form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
   form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
   expect(invoke.mock.calls.filter(([command])=>command==='parse_quick_add')).toHaveLength(1);
   expect(form.getAttribute('aria-busy')).toBe('true');
   finishParse({title:'Task',body:'',column:'backlog',labels:[],due:'2026-09-06',start:'2026-09-05',confidence:1,warnings:[]});
   await vi.waitFor(()=>expect(state.cards).toHaveLength(1));
   expect(invoke).toHaveBeenCalledWith('add_card',{path:'/board',input:{title:'Task',body:'',column:'backlog',labels:[],due:'2026-09-06',start:'2026-09-05',position:1000}});
   setInvokeForTests(null);
 });
});

describe('OpenAI-compatible provider',()=>{
 it('loads OpenAI-compatible models with the configured base URL and key',async()=>{
  state.openaiModels=[];state.modelSettings={openai_api_key:'sk-saved'};state.error='';state.modelLoading=false;
  const models=[{id:'anthropic/claude-sonnet-4',created:1,owned_by:'anthropic'},{id:'openai/gpt-4o'}];
  const invoke=vi.fn(async(command:string)=>{if(command==='list_openai_models')return models;throw new Error(command)});
  setInvokeForTests(invoke as any);
  await loadOpenAIModels('https://openrouter.ai/api/v1','sk-test');
  expect(invoke).toHaveBeenCalledWith('list_openai_models',{baseUrl:'https://openrouter.ai/api/v1',apiKey:'sk-test'});
  expect(state.openaiModels).toEqual(models);
  expect(state.modelSettings.openai_base_url).toBe('https://openrouter.ai/api/v1');
  expect(state.error).toBe('');
  setInvokeForTests(async()=>{throw new Error('unauthorized')});
  await loadOpenAIModels();
  expect(state.error).toContain('unauthorized');
  setInvokeForTests(null);
 });
 it('fetches OpenAI models during loadModels when the provider is openai, tolerating list failures',async()=>{
  state.openaiModels=[];state.error='';
  const settings={provider:'openai' as const,model_id:'openai/gpt-4o',openai_api_key:'sk-x',openai_base_url:'https://openrouter.ai/api/v1'};
  const invoke=vi.fn(async(command:string)=>{if(command==='model_settings')return settings;if(command==='list_local_models')return [];if(command==='list_openai_models')return [{id:'openai/gpt-4o'}];throw new Error(command)});
  setInvokeForTests(invoke as any);
  await loadModels();
  expect(invoke).toHaveBeenCalledWith('list_openai_models',{baseUrl:'https://openrouter.ai/api/v1',apiKey:'sk-x'});
  expect(state.openaiModels[0]?.id).toBe('openai/gpt-4o');
  setInvokeForTests((async(command:string)=>{if(command==='model_settings')return settings;if(command==='list_local_models')return [];throw new Error('offline')}) as any);
  await loadModels();
  expect(state.error).toContain('offline');
  setInvokeForTests(null);
 });
 it('shows the OpenAI API provider tab and preserves saved connection fields when switching',async()=>{
  state.view='settings';state.error='';state.localModels=[];state.openaiModels=[];state.modelLoading=false;
  state.modelSettings={provider:'ollama',ollama_url:'http://127.0.0.1:11434',openai_base_url:'https://openrouter.ai/api/v1',openai_api_key:'sk-saved'};
  setInvokeForTests((async()=>[]) as any);render();
  const tab=document.querySelector<HTMLElement>('[data-model-provider="openai"]')!;
  expect(tab.textContent).toContain('OpenAI API');
  tab.click();
  expect(state.modelSettings.provider).toBe('openai');
  expect(state.modelSettings.ollama_url).toBe('http://127.0.0.1:11434');
  expect(state.modelSettings.openai_base_url).toBe('https://openrouter.ai/api/v1');
  expect(state.modelSettings.openai_api_key).toBe('sk-saved');
  setInvokeForTests(null);
 });
 it('renders the OpenAI panel, connects with edited inputs, and selects a listed model',async()=>{
  state.view='settings';state.error='';state.modelLoading=false;
  state.openaiModels=[{id:'anthropic/claude-sonnet-4',owned_by:'anthropic'},{id:'openai/gpt-4o'}];
  state.modelSettings={provider:'openai',openai_base_url:'https://openrouter.ai/api/v1',openai_api_key:'sk-saved'};
  const invoke=vi.fn(async(command:string)=>{if(command==='save_model_settings')return null;if(command==='model_settings')return state.modelSettings;if(command==='list_local_models')return [];if(command==='list_openai_models')return state.openaiModels;throw new Error(command)});
  setInvokeForTests(invoke as any);render();
  expect(document.querySelector<HTMLInputElement>('#openai-base-url')?.value).toBe('https://openrouter.ai/api/v1');
  const keyInput=document.querySelector<HTMLInputElement>('#openai-api-key')!;
  expect(keyInput.type).toBe('password');expect(keyInput.value).toBe('sk-saved');
  document.querySelector<HTMLInputElement>('#openai-base-url')!.value='https://api.example.com/v1';
  keyInput.value='sk-edited';
  const connect=document.querySelector<HTMLElement>('#refresh-openai')!;
  expect(connect.textContent).toContain('Connect');
  connect.click();
  await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('list_openai_models',{baseUrl:'https://api.example.com/v1',apiKey:'sk-edited'}));
  const row=document.querySelector<HTMLElement>('[data-select-openai="anthropic/claude-sonnet-4"]')!;
  expect(row.classList.contains('model-row')).toBe(true);
  expect(row.textContent).toContain('OpenAI-compatible API');
  row.click();
  await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('save_model_settings',{settings:expect.objectContaining({provider:'openai',model_id:'anthropic/claude-sonnet-4',openai_base_url:'https://api.example.com/v1',openai_api_key:'sk-edited'})}));
  setInvokeForTests(null);
 });
 it('saves a manually entered OpenAI model id and rejects empty input',async()=>{
  state.view='settings';state.error='';state.modelLoading=false;state.openaiModels=[];
  state.modelSettings={provider:'openai',openai_base_url:'https://openrouter.ai/api/v1',openai_api_key:'sk-saved'};
  const invoke=vi.fn(async(command:string)=>{if(command==='save_model_settings')return null;if(command==='model_settings')return state.modelSettings;if(command==='list_local_models')return [];if(command==='list_openai_models')return [];throw new Error(command)});
  setInvokeForTests(invoke as any);render();
  const input=document.querySelector<HTMLInputElement>('#openai-model')!;
  expect(input.placeholder).toContain('anthropic/claude-sonnet-4');
  document.querySelector<HTMLElement>('#use-openai-model')!.click();
  expect(state.error).not.toBe('');
  expect(invoke).not.toHaveBeenCalledWith('save_model_settings',expect.anything());
  document.querySelector<HTMLInputElement>('#openai-model')!.value='  openai/gpt-4o-mini  ';
  document.querySelector<HTMLElement>('#use-openai-model')!.click();
  await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('save_model_settings',{settings:expect.objectContaining({provider:'openai',model_id:'openai/gpt-4o-mini',openai_base_url:'https://openrouter.ai/api/v1',openai_api_key:'sk-saved'})}));
  setInvokeForTests(null);
 });
 it('shows an API key hint instead of model rows until a key is saved',()=>{
  state.view='settings';state.error='';state.modelLoading=false;state.openaiModels=[];
  state.modelSettings={provider:'openai'};
  render();
  expect(document.querySelector('[data-select-openai]')).toBeNull();
  expect(document.querySelector('.empty-models')?.textContent).toContain('API key');
 });
 it('hides the keep-model-loaded toggle for the OpenAI provider',()=>{
  state.view='settings';state.error='';state.modelLoading=false;state.openaiModels=[];
  state.modelSettings={provider:'openai',model_id:'openai/gpt-4o',openai_api_key:'sk'};
  render();
  expect(document.querySelector('#keep-model-loaded')).toBeNull();
  state.modelSettings={provider:'huggingface',model_id:'local.gguf'};
  state.localModels=[{id:'local.gguf',provider:'huggingface',name:'Local',source:'repo',size_bytes:1}];
  render();
  expect(document.querySelector('#keep-model-loaded')).not.toBeNull();
 });
 it('resolves OpenAI model ids in the status pill',()=>{
  state.view='settings';state.error='';state.modelLoading=false;
  state.modelSettings={provider:'openai',model_id:'anthropic/claude-sonnet-4',openai_api_key:'sk'};
  state.openaiModels=[{id:'anthropic/claude-sonnet-4',owned_by:'anthropic'}];
  render();
  expect(document.querySelector('.model-setup-status')?.textContent).toContain('Ready');
  expect(document.querySelector('.model-setup-status')?.textContent).toContain('anthropic/claude-sonnet-4');
 });
 it('checks OpenAI API access with the entered endpoint and key',async()=>{
  state.modelSettings={provider:'openai',openai_base_url:'https://openrouter.ai/api/v1',openai_api_key:'sk-saved'};state.modelFeedback='';state.error='';
  const invoke=vi.fn(async(command:string)=>{if(command==='check_openai_access'||command==='save_model_settings')return null;throw new Error(command)});setInvokeForTests(invoke as any);
  await checkOpenAIAccess('https://api.example.com/v1','sk-test');
  expect(invoke).toHaveBeenCalledWith('check_openai_access',{baseUrl:'https://api.example.com/v1',apiKey:'sk-test'});
  expect(invoke).toHaveBeenCalledWith('save_model_settings',{settings:expect.objectContaining({openai_base_url:'https://api.example.com/v1',openai_api_key:'sk-test'})});
  expect(state.modelFeedback).toContain('Access confirmed');expect(state.error).toBe('');
  setInvokeForTests(async()=>{throw new Error('401 Unauthorized')});await checkOpenAIAccess();
  expect(state.error).toContain('401 Unauthorized');setInvokeForTests(null);
 });
 it('wires Check access to the edited OpenAI credentials',async()=>{
  state.view='settings';state.error='';state.modelLoading=false;state.modelSettings={provider:'openai',openai_base_url:'https://openrouter.ai/api/v1',openai_api_key:'sk-saved'};
  const invoke=vi.fn(async(command:string)=>{if(command==='check_openai_access'||command==='save_model_settings')return null;throw new Error(command)});setInvokeForTests(invoke as any);render();
  document.querySelector<HTMLInputElement>('#openai-base-url')!.value='https://api.example.com/v1';document.querySelector<HTMLInputElement>('#openai-api-key')!.value='sk-edited';
  document.querySelector<HTMLElement>('#check-openai-access')!.click();
  await vi.waitFor(()=>expect(invoke).toHaveBeenCalledWith('check_openai_access',{baseUrl:'https://api.example.com/v1',apiKey:'sk-edited'}));
  await vi.waitFor(()=>expect(document.querySelector('.model-feedback')?.textContent).toContain('Access confirmed'));setInvokeForTests(null);
 });
 it('opens Quick Add with an OpenAI provider and model selected',()=>{
  state.boardPath='/board';state.view='board';state.error='';state.quickAddOpen=false;state.selected=null;state.cards=[];
  state.columns=[{id:'backlog',name:'Backlog'}];
  state.modelSettings={provider:'openai',model_id:'anthropic/claude-sonnet-4',openai_api_key:'sk'};
  render();
  document.querySelector<HTMLElement>('#new-card')!.click();
  expect(state.quickAddOpen).toBe(true);
  expect(document.querySelector('#quick-add-form')).not.toBeNull();
  state.quickAddOpen=false;
 });
});
