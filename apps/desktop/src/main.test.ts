import { describe, expect, it, beforeEach, vi } from 'vitest';
import { renderCardText } from './testables';
import { state, normalize, applyBoardInfo, loadCards, pairAddress, syncNow, openBoard, setInvokeForTests, setDirectoryPickerForTests } from './main';

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
 it('opens a board selected by the native directory picker',async()=>{
   setDirectoryPickerForTests(async create=>{expect(create).toBe(false);return '/chosen-board'});
   const invoke=vi.fn(async(command:string)=>{if(command==='open_board')return {path:'/chosen-board',board_id:'01BOARDULID',cards:[]};if(command==='watch_board')return true;if(command==='list_cards')return [];if(command==='sync_status')return {connected:false,trusted_peers:0,connection:'offline'};if(command==='endpoint_info')return {endpoint_id:'local',address:'local-address'};if(command==='list_trusted_peers')return [];throw new Error(command)});
   setInvokeForTests(invoke as any);await openBoard(false);
   expect(invoke).toHaveBeenCalledWith('open_board',{path:'/chosen-board'});
   expect(state.boardPath).toBe('/chosen-board');expect(state.boardId).toBe('01BOARDULID');expect(state.error).toBe('');
   setDirectoryPickerForTests(null);setInvokeForTests(null);
 });
});
