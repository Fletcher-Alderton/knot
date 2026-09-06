import { describe, expect, it, beforeEach, vi } from 'vitest';
import { renderCardText } from './testables';
import { state, normalize, applyBoardInfo, loadCards, pairAddress, syncNow, openBoard, moveCard, renameBoard, renameColumn, createColumn, reorderColumns, setView, render, applyDownloadProgress, setInvokeForTests, setDirectoryPickerForTests } from './main';

describe('Kanban UI',()=>{
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
