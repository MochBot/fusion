"""TTRM replay preprocessor — converts .ttrm replays into flat binary training samples.

Simulates TETR.IO game mechanics from initial board snapshot + key events to
reconstruct board state at each piece placement. Extracts 854-feature vectors
+ 5 labels per placement, writes as contiguous f32 binary chunks.

Feature layout (854 floats):
  [0..400)    player board  (10×40, column-major, 1.0=filled 0.0=empty)
  [400..800)  opponent board (10×40, same layout)
  [800..849)  piece one-hots (7 pieces × 7 slots: current, hold, queue[0..4])
  [849..854)  scalars: combo_norm, b2b_norm, lines_norm, garbage_pending_norm, bag_position_norm

Label layout (5 floats):
  game_outcome      (1.0=win, 0.0=loss)
  lines_sent         (normalized by total lines)
  b2b_after          (normalized b2b chain)
  position_normalized (how far into the game, 0..1)
  time_to_topout     (frames remaining / total frames, 1..0)
"""

from __future__ import annotations

import json
import multiprocessing as mp
import os
import sys
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np

try:
    from ..utils.config import (
        PIECES_PER_BAG,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        FLOATS_PER_SAMPLE,
    )
    from ..utils.example_schema import (
        SCHEMA_VERSION,
        CanonicalExample,
        ExampleIdentity,
        PIECE_INDEX,
        PIECE_ORDER,
        flatten_example,
        group_ids_path,
        load_dataset_metadata,
        stable_group_hash,
        validate_example,
        write_dataset_metadata,
    )
    from ..utils.policy_value_schema import (
        PHASE1_SCHEMA_VERSION,
        PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
        PolicyValuePlayerContext,
        PolicyValueOracleRequest,
        encode_move_raw,
        load_player_context_metadata,
        policy_value_player_context_path,
        policy_value_requests_path,
        serialize_player_context,
        serialize_policy_value_oracle_request,
        write_player_context_metadata,
    )
except ImportError:
    TRAINING_ROOT = Path(__file__).resolve().parents[1]
    if str(TRAINING_ROOT) not in sys.path:
        sys.path.insert(0, str(TRAINING_ROOT))
    from utils.config import (
        PIECES_PER_BAG,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        FLOATS_PER_SAMPLE,
    )
    from utils.example_schema import (
        SCHEMA_VERSION,
        CanonicalExample,
        ExampleIdentity,
        PIECE_INDEX,
        PIECE_ORDER,
        flatten_example,
        group_ids_path,
        load_dataset_metadata,
        stable_group_hash,
        validate_example,
        write_dataset_metadata,
    )
    from utils.policy_value_schema import (
        PHASE1_SCHEMA_VERSION,
        PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
        PolicyValuePlayerContext,
        PolicyValueOracleRequest,
        encode_move_raw,
        load_player_context_metadata,
        policy_value_player_context_path,
        policy_value_requests_path,
        serialize_player_context,
        serialize_policy_value_oracle_request,
        write_player_context_metadata,
    )

PIECE_NAMES = list(PIECE_ORDER)
PLAYER_CONTEXT_RECENT_HORIZON = 7
PLAYER_CONTEXT_FUTURE_HORIZON = 14


@dataclass(slots=True)
class _PlayerPlacementEvent:
    replay_id: str
    round_id: int
    player_id: int
    frame_id: int
    group_id: str
    spawn_piece: str
    actual_piece: str
    actual_move_raw: int
    actual_x: int
    actual_y: int
    actual_rotation: int
    actual_hold_used: bool
    actual_lines_cleared: int
    input_keys: list[str]
    hold_piece: str | None
    queue: list[str]

# SRS piece shapes — each rotation state is list of (row, col) offsets from spawn origin
# Origin is top-left of bounding box. Coordinates are (row_offset, col_offset).
SRS_SHAPES: dict[str, list[list[tuple[int, int]]]] = {
    "i": [
        [(1, 0), (1, 1), (1, 2), (1, 3)],
        [(0, 2), (1, 2), (2, 2), (3, 2)],
        [(2, 0), (2, 1), (2, 2), (2, 3)],
        [(0, 1), (1, 1), (2, 1), (3, 1)],
    ],
    "j": [
        [(0, 0), (1, 0), (1, 1), (1, 2)],
        [(0, 1), (0, 2), (1, 1), (2, 1)],
        [(1, 0), (1, 1), (1, 2), (2, 2)],
        [(0, 1), (1, 1), (2, 0), (2, 1)],
    ],
    "l": [
        [(0, 2), (1, 0), (1, 1), (1, 2)],
        [(0, 1), (1, 1), (2, 1), (2, 2)],
        [(1, 0), (1, 1), (1, 2), (2, 0)],
        [(0, 0), (0, 1), (1, 1), (2, 1)],
    ],
    "o": [
        [(0, 1), (0, 2), (1, 1), (1, 2)],
        [(0, 1), (0, 2), (1, 1), (1, 2)],
        [(0, 1), (0, 2), (1, 1), (1, 2)],
        [(0, 1), (0, 2), (1, 1), (1, 2)],
    ],
    "s": [
        [(0, 1), (0, 2), (1, 0), (1, 1)],
        [(0, 1), (1, 1), (1, 2), (2, 2)],
        [(1, 1), (1, 2), (2, 0), (2, 1)],
        [(0, 0), (1, 0), (1, 1), (2, 1)],
    ],
    "t": [
        [(0, 1), (1, 0), (1, 1), (1, 2)],
        [(0, 1), (1, 1), (1, 2), (2, 1)],
        [(1, 0), (1, 1), (1, 2), (2, 1)],
        [(0, 1), (1, 0), (1, 1), (2, 1)],
    ],
    "z": [
        [(0, 0), (0, 1), (1, 1), (1, 2)],
        [(0, 2), (1, 1), (1, 2), (2, 1)],
        [(1, 0), (1, 1), (2, 1), (2, 2)],
        [(0, 1), (1, 0), (1, 1), (2, 0)],
    ],
}

# SRS wall kick tables
# (old_rotation, new_rotation) -> list of (dx, dy) offsets to try
JLSTZ_KICKS: dict[tuple[int, int], list[tuple[int, int]]] = {
    (0, 1): [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
    (1, 0): [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
    (1, 2): [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
    (2, 1): [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
    (2, 3): [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
    (3, 2): [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    (3, 0): [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    (0, 3): [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
}

I_KICKS: dict[tuple[int, int], list[tuple[int, int]]] = {
    (0, 1): [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
    (1, 0): [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
    (1, 2): [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
    (2, 1): [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
    (2, 3): [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
    (3, 2): [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
    (3, 0): [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
    (0, 3): [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
}


class GameState:
    """Minimal TETR.IO game state simulator for feature extraction."""

    def __init__(self, full_event_data: dict[str, Any]) -> None:
        game = full_event_data["game"]

        # board: 40×10, True = filled
        self.board = np.zeros((BOARD_HEIGHT, BOARD_WIDTH), dtype=np.float32)
        raw_board = game.get("board", [])
        for row_idx, row in enumerate(raw_board):
            if row_idx >= BOARD_HEIGHT:
                break
            if isinstance(row, list):
                for col_idx, cell in enumerate(row):
                    if col_idx >= BOARD_WIDTH:
                        break
                    if cell is not None:
                        self.board[row_idx][col_idx] = 1.0

        # piece queue from bag
        self.queue: list[str] = []
        bag = game.get("bag", [])
        for p in bag:
            if isinstance(p, str) and p.lower() in PIECE_INDEX:
                self.queue.append(p.lower())

        # hold piece
        hold_data = game.get("hold", {})
        self.hold: str | None = None
        self.hold_locked = False
        if isinstance(hold_data, dict):
            hp = hold_data.get("piece")
            if hp and isinstance(hp, str):
                self.hold = hp.lower()
            self.hold_locked = bool(hold_data.get("locked", False))

        # current falling piece
        falling = game.get("falling", {})
        self.current_piece: str | None = None
        self.piece_x = 3
        self.piece_y = 0
        self.piece_r = 0
        if isinstance(falling, dict):
            ptype = falling.get("type")
            if ptype and isinstance(ptype, str):
                self.current_piece = ptype.lower()
            self.piece_x = int(falling.get("x", 3))
            self.piece_y = int(falling.get("y", 0))
            self.piece_r = int(falling.get("r", 0)) % 4
        if self.current_piece is not None and self.queue and self.queue[0] == self.current_piece:
            self.queue.pop(0)

        # stats
        stats = full_event_data.get("stats", {})
        self.combo = int(stats.get("combo", 0))
        self.b2b = int(stats.get("btb", 0))
        self.lines = int(stats.get("lines", 0))
        self.pieces_placed = int(stats.get("piecesplaced", 0))
        self.garbage_pending = 0

        # bag tracking for bag_position
        self.bag_number = 0

    def _get_cells(self, piece: str, rotation: int) -> list[tuple[int, int]]:
        p = piece.lower()
        if p not in SRS_SHAPES:
            return []
        return SRS_SHAPES[p][rotation % 4]

    def _valid_position(self, piece: str, x: int, y: int, r: int) -> bool:
        for dr, dc in self._get_cells(piece, r):
            row, col = y + dr, x + dc
            if row < 0 or row >= BOARD_HEIGHT or col < 0 or col >= BOARD_WIDTH:
                return False
            if self.board[row][col] > 0.5:
                return False
        return True

    def _try_rotate(self, direction: int) -> None:
        if self.current_piece is None:
            return
        old_r = self.piece_r
        new_r = (old_r + direction) % 4
        kicks = I_KICKS if self.current_piece == "i" else JLSTZ_KICKS
        kick_key = (old_r, new_r)
        if kick_key not in kicks:
            return
        for dx, dy in kicks[kick_key]:
            nx, ny = self.piece_x + dx, self.piece_y - dy  # SRS: positive dy = up
            if self._valid_position(self.current_piece, nx, ny, new_r):
                self.piece_x, self.piece_y, self.piece_r = nx, ny, new_r
                return

    def _move(self, dx: int) -> None:
        if self.current_piece is None:
            return
        nx = self.piece_x + dx
        if self._valid_position(self.current_piece, nx, self.piece_y, self.piece_r):
            self.piece_x = nx

    def _soft_drop(self) -> None:
        if self.current_piece is None:
            return
        while self._valid_position(self.current_piece, self.piece_x, self.piece_y + 1, self.piece_r):
            self.piece_y += 1

    def _hard_drop(self) -> int:
        """Drop and lock piece, clear lines. Returns lines cleared."""
        if self.current_piece is None:
            return 0

        # drop to bottom
        while self._valid_position(self.current_piece, self.piece_x, self.piece_y + 1, self.piece_r):
            self.piece_y += 1

        # lock piece
        for dr, dc in self._get_cells(self.current_piece, self.piece_r):
            row, col = self.piece_y + dr, self.piece_x + dc
            if 0 <= row < BOARD_HEIGHT and 0 <= col < BOARD_WIDTH:
                self.board[row][col] = 1.0

        # clear lines
        lines_cleared = 0
        new_board = np.zeros_like(self.board)
        write_row = BOARD_HEIGHT - 1
        for read_row in range(BOARD_HEIGHT - 1, -1, -1):
            if np.all(self.board[read_row] > 0.5):
                lines_cleared += 1
            else:
                new_board[write_row] = self.board[read_row]
                write_row -= 1
        self.board = new_board

        self.lines += lines_cleared
        self.pieces_placed += 1
        self.bag_number = self.pieces_placed // PIECES_PER_BAG
        if lines_cleared > 0:
            self.combo += 1
            if lines_cleared >= 4:
                self.b2b += 1
            else:
                self.b2b = 0
        else:
            self.combo = 0

        # spawn next piece
        self._spawn_next()
        return lines_cleared

    def preview_hard_drop_y(self) -> int:
        if self.current_piece is None:
            raise ValueError("current_piece is required to preview hard drop")
        preview_y = self.piece_y
        while self._valid_position(self.current_piece, self.piece_x, preview_y + 1, self.piece_r):
            preview_y += 1
        return preview_y

    def _do_hold(self) -> None:
        if self.current_piece is None or self.hold_locked:
            return
        old_hold = self.hold
        self.hold = self.current_piece
        self.hold_locked = True
        if old_hold is not None:
            self.current_piece = old_hold
            self._reset_piece_position()
        else:
            self._spawn_next()

    def _spawn_next(self) -> None:
        self.hold_locked = False
        if self.queue:
            self.current_piece = self.queue.pop(0)
        else:
            self.current_piece = None
        self._reset_piece_position()

    def _reset_piece_position(self) -> None:
        self.piece_x = 3
        self.piece_y = 0
        self.piece_r = 0

    def process_key(self, key: str) -> int | None:
        """Process a key event. Returns lines cleared on hardDrop, else None."""
        if key == "moveLeft":
            self._move(-1)
        elif key == "moveRight":
            self._move(1)
        elif key == "rotateCW":
            self._try_rotate(1)
        elif key == "rotateCCW":
            self._try_rotate(-1)
        elif key == "rotate180":
            self._try_rotate(2)
        elif key == "softDrop":
            self._soft_drop()
        elif key == "hardDrop":
            return self._hard_drop()
        elif key == "hold":
            self._do_hold()
        return None

    def to_canonical_example(
        self,
        *,
        replay_id: str,
        round_id: int,
        player_id: int,
        frame_id: int,
        opponent_board: np.ndarray,
        game_outcome: float,
        lines_sent: float,
        position_normalized: float,
        time_to_topout: float,
    ) -> CanonicalExample:
        example = CanonicalExample(
            identity=ExampleIdentity(
                schema_version=SCHEMA_VERSION,
                replay_id=replay_id,
                round_id=round_id,
                player_id=player_id,
                frame_id=frame_id,
                group_id=f"{replay_id}:round:{round_id}",
            ),
            player_board=self.board.copy(),
            opponent_board=opponent_board.copy(),
            current_piece=self.current_piece,
            hold_piece=self.hold,
            queue=tuple(self.queue[:5]),
            combo=self.combo,
            b2b=self.b2b,
            lines_cleared_total=self.lines,
            pending_garbage=self.garbage_pending,
            bag_number=self.bag_number,
            game_outcome=game_outcome,
            lines_sent=lines_sent,
            b2b_after=min(self.b2b / 10.0, 1.0),
            position_normalized=position_normalized,
            time_to_topout=max(time_to_topout, 0.0),
        )
        validate_example(example)
        return example

    def to_policy_value_oracle_request(
        self,
        *,
        replay_id: str,
        round_id: int,
        player_id: int,
        frame_id: int,
        opponent_board: np.ndarray,
    ) -> PolicyValueOracleRequest:
        if self.current_piece is None:
            raise ValueError("current_piece is required for policy/value oracle request")
        return PolicyValueOracleRequest(
            schema_version=PHASE1_SCHEMA_VERSION,
            replay_id=replay_id,
            round_id=round_id,
            player_id=player_id,
            frame_id=frame_id,
            group_id=f"{replay_id}:round:{round_id}",
            player_board_rows=encode_board_rows(self.board),
            opponent_board_rows=encode_board_rows(opponent_board),
            current_piece=self.current_piece,
            hold_piece=self.hold,
            queue=list(self.queue[:5]),
            combo=self.combo,
            b2b=self.b2b,
            lines=self.lines,
            pending_garbage=self.garbage_pending,
            bag_number=self.bag_number,
        )


def encode_board_rows(board: np.ndarray) -> list[int]:
    rows: list[int] = []
    for y in range(BOARD_HEIGHT):
        value = 0
        for x in range(BOARD_WIDTH):
            if board[y][x] > 0.5:
                value |= 1 << x
        rows.append(value)
    return rows


def process_round(
    round_data: list[dict[str, Any]],
    replay_id: str,
    round_id: int,
) -> tuple[list[CanonicalExample], list[PolicyValueOracleRequest], list[PolicyValuePlayerContext]]:
    """Process one round into canonical decision-point examples."""
    if len(round_data) < 2:
        return [], [], []

    # determine winner from end events
    outcomes = [0.0, 0.0]
    total_frames = [1, 1]
    end_stats: list[dict[str, Any]] = [{}, {}]

    for pi in range(2):
        events = round_data[pi]["replay"]["events"]
        total_frames[pi] = max(round_data[pi]["replay"].get("frames", 1), 1)
        for e in events:
            if e.get("type") == "end":
                d = e.get("data", {})
                if d.get("gameoverreason") == "winner":
                    outcomes[pi] = 1.0
                end_stats[pi] = d.get("stats", {})

    # init game states from full events
    states: list[GameState | None] = [None, None]
    for pi in range(2):
        events = round_data[pi]["replay"]["events"]
        for e in events:
            if e.get("type") == "full":
                states[pi] = GameState(e["data"])
                break

    if states[0] is None or states[1] is None:
        return [], [], []

    # process both players frame-by-frame so opponent snapshots are aligned to the same replay instant.
    keydowns_per_player = []
    total_placements = []
    total_lines_sent = []
    for pi in range(2):
        events = round_data[pi]["replay"]["events"]
        keydowns = [
            e for e in events
            if e.get("type") == "keydown" and isinstance(e.get("data"), dict)
        ]
        keydowns_per_player.append(keydowns)
        total_placements.append(sum(1 for kd in keydowns if kd["data"].get("key") == "hardDrop"))
        garbage_stats = end_stats[pi].get("garbage")
        total_lines_sent.append(float(garbage_stats.get("sent", 0) if isinstance(garbage_stats, dict) else 0))

    indices = [0, 0]
    placement_indices = [0, 0]
    samples: list[CanonicalExample] = []
    requests: list[PolicyValueOracleRequest] = []
    player_events: list[list[_PlayerPlacementEvent]] = [[], []]
    ordered_context_indices: list[tuple[int, int]] = []
    turn_keys: list[list[str]] = [[], []]
    turn_start_piece: list[str | None] = [None, None]

    while indices[0] < len(keydowns_per_player[0]) or indices[1] < len(keydowns_per_player[1]):
        next_frames = [
            keydowns_per_player[pi][indices[pi]].get("frame", 0)
            for pi in range(2)
            if indices[pi] < len(keydowns_per_player[pi])
        ]
        current_frame = min(next_frames)
        opponent_snapshots = [states[0].board.copy(), states[1].board.copy()]  # type: ignore[union-attr]

        for pi in range(2):
            state = states[pi]
            assert state is not None
            opp = 1 - pi
            while indices[pi] < len(keydowns_per_player[pi]) and keydowns_per_player[pi][indices[pi]].get("frame", 0) == current_frame:
                kd = keydowns_per_player[pi][indices[pi]]
                key = kd["data"].get("key", "")
                frame = int(kd.get("frame", 0))
                if not turn_keys[pi]:
                    turn_start_piece[pi] = state.current_piece
                turn_keys[pi].append(key)
                oracle_request = None
                placement_event = None
                if key == "hardDrop":
                    if state.current_piece is None:
                        indices[pi] = len(keydowns_per_player[pi])
                        break
                    oracle_request = state.to_policy_value_oracle_request(
                        replay_id=replay_id,
                        round_id=round_id,
                        player_id=pi,
                        frame_id=frame,
                        opponent_board=opponent_snapshots[opp],
                    )
                    placement_event = _PlayerPlacementEvent(
                        replay_id=replay_id,
                        round_id=round_id,
                        player_id=pi,
                        frame_id=frame,
                        group_id=f"{replay_id}:round:{round_id}",
                        spawn_piece=turn_start_piece[pi] or state.current_piece,
                        actual_piece=state.current_piece,
                        actual_move_raw=encode_move_raw(
                            piece=state.current_piece,
                            x=state.piece_x,
                            y=state.preview_hard_drop_y(),
                            rotation=state.piece_r,
                        ),
                        actual_x=state.piece_x,
                        actual_y=state.preview_hard_drop_y(),
                        actual_rotation=state.piece_r,
                        actual_hold_used="hold" in turn_keys[pi],
                        actual_lines_cleared=0,
                        input_keys=list(turn_keys[pi]),
                        hold_piece=state.hold,
                        queue=list(state.queue[:5]),
                    )
                result = state.process_key(key)
                if result is not None:
                    if oracle_request is not None:
                        requests.append(oracle_request)
                    if placement_event is not None:
                        placement_event.actual_lines_cleared = result
                        player_events[pi].append(placement_event)
                        ordered_context_indices.append((pi, len(player_events[pi]) - 1))
                    position_norm = placement_indices[pi] / max(total_placements[pi], 1)
                    time_to_topout = 1.0 - (frame / total_frames[pi])
                    samples.append(
                        state.to_canonical_example(
                            replay_id=replay_id,
                            round_id=round_id,
                            player_id=pi,
                            frame_id=frame,
                            opponent_board=opponent_snapshots[opp],
                            game_outcome=outcomes[pi],
                            lines_sent=min(total_lines_sent[pi] / 40.0, 1.0),
                            position_normalized=position_norm,
                            time_to_topout=time_to_topout,
                        )
                    )
                    placement_indices[pi] += 1
                    turn_keys[pi].clear()
                    turn_start_piece[pi] = None
                indices[pi] += 1

    contexts: list[PolicyValuePlayerContext] = []
    for pi, event_index in ordered_context_indices:
        events = player_events[pi]
        event = events[event_index]
        recent_events = events[max(0, event_index - PLAYER_CONTEXT_RECENT_HORIZON):event_index]
        future_events = events[event_index + 1:event_index + 1 + PLAYER_CONTEXT_FUTURE_HORIZON]
        contexts.append(
            PolicyValuePlayerContext(
                schema_version=PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
                replay_id=event.replay_id,
                round_id=event.round_id,
                player_id=event.player_id,
                frame_id=event.frame_id,
                group_id=event.group_id,
                spawn_piece=event.spawn_piece,
                actual_piece=event.actual_piece,
                actual_move_raw=event.actual_move_raw,
                actual_x=event.actual_x,
                actual_y=event.actual_y,
                actual_rotation=event.actual_rotation,
                actual_hold_used=event.actual_hold_used,
                actual_lines_cleared=event.actual_lines_cleared,
                input_keys=list(event.input_keys),
                hold_piece=event.hold_piece,
                queue=list(event.queue),
                recent_piece_sequence=[placement.actual_piece for placement in recent_events],
                future_piece_sequence=[placement.actual_piece for placement in future_events],
                recent_hold_usage=[placement.actual_hold_used for placement in recent_events],
                future_hold_usage=[placement.actual_hold_used for placement in future_events],
            )
        )

    return samples, requests, contexts


def process_file_artifacts(filepath: str | Path) -> tuple[list[CanonicalExample], list[PolicyValueOracleRequest], list[PolicyValuePlayerContext]]:
    with open(filepath) as f:
        data = json.load(f)

    replay_id = Path(filepath).stem
    all_samples: list[CanonicalExample] = []
    all_requests: list[PolicyValueOracleRequest] = []
    all_contexts: list[PolicyValuePlayerContext] = []
    rounds = data.get("replay", {}).get("rounds", [])

    for round_index, round_data in enumerate(rounds):
        if isinstance(round_data, list) and len(round_data) >= 2:
            samples, requests, contexts = process_round(round_data, replay_id=replay_id, round_id=round_index)
            all_samples.extend(samples)
            all_requests.extend(requests)
            all_contexts.extend(contexts)

    return all_samples, all_requests, all_contexts


def process_file(filepath: str | Path) -> list[CanonicalExample]:
    """Process a single .ttrm file, return all canonical training examples."""
    samples, _, _ = process_file_artifacts(filepath)
    return samples


def _process_file_worker(filepath: str) -> tuple[bytes, bytes, bytes, bytes, int, str | None]:
    """Worker function for multiprocessing — must be top-level and picklable.

    Returns (raw_bytes, sample_count, error_or_None).
    """
    try:
        samples, requests, contexts = process_file_artifacts(filepath)
        if not samples:
            return b"", b"", b"", b"", 0, None
        buf = bytearray()
        groups = bytearray()
        for sample in samples:
            flat = flatten_example(sample)
            groups.extend(np.uint64(stable_group_hash(sample.identity.group_id)).tobytes())
            assert flat.shape == (FLOATS_PER_SAMPLE,), f"Bad shape: {flat.shape}"
            buf.extend(flat.tobytes())
        request_buf = bytearray()
        for request in requests:
            request_buf.extend(serialize_policy_value_oracle_request(request).encode("utf-8"))
            request_buf.extend(b"\n")
        context_buf = bytearray()
        for context in contexts:
            context_buf.extend(serialize_player_context(context).encode("utf-8"))
            context_buf.extend(b"\n")
        return bytes(buf), bytes(groups), bytes(request_buf), bytes(context_buf), len(samples), None
    except Exception as exc:
        return b"", b"", b"", b"", 0, f"{Path(filepath).name}: {exc}"


def split_replay_files(
    input_dir: str | Path,
    *,
    shard_count: int,
    max_files: int | None = None,
) -> list[list[Path]]:
    input_dir = Path(input_dir)
    replay_files = sorted(input_dir.rglob("*.ttrm"))
    if max_files is not None:
        replay_files = replay_files[:max_files]
    if not replay_files:
        return []
    if shard_count <= 0:
        raise ValueError(f"shard_count must be positive, got {shard_count}")

    resolved_shard_count = min(shard_count, len(replay_files))
    target_counts = [len(replay_files) // resolved_shard_count] * resolved_shard_count
    for index in range(len(replay_files) % resolved_shard_count):
        target_counts[index] += 1

    shards: list[list[Path]] = []
    start = 0
    for count in target_counts:
        shards.append(replay_files[start:start + count])
        start += count
    return shards


def preprocess_replay_files(
    replay_files: Sequence[str | Path],
    output_path: str | Path,
    *,
    num_workers: int | None = None,
) -> int:
    output_path = Path(output_path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    normalized_files = [str(Path(path)) for path in replay_files]

    if num_workers is None:
        num_workers = os.cpu_count() or 1

    total_files = len(normalized_files)
    total_samples = 0
    errors = 0

    print(f"processing {total_files} files with {num_workers} workers...")

    with open(output_path, "wb") as out, open(group_ids_path(output_path), "wb") as groups_out, open(policy_value_requests_path(output_path), "wb") as requests_out, open(policy_value_player_context_path(output_path), "wb") as contexts_out:
        with mp.Pool(processes=num_workers) as pool:
            for i, (raw_bytes, group_bytes, request_bytes, context_bytes, count, err) in enumerate(
                pool.imap_unordered(_process_file_worker, normalized_files, chunksize=10)
            ):
                if err:
                    print(f"  SKIP {err}", file=sys.stderr)
                    errors += 1
                    continue
                if raw_bytes:
                    out.write(raw_bytes)
                    groups_out.write(group_bytes)
                    requests_out.write(request_bytes)
                    contexts_out.write(context_bytes)
                total_samples += count
                if (i + 1) % 100 == 0:
                    print(f"  processed {i + 1}/{total_files} files, {total_samples} samples, {errors} errors")

    write_dataset_metadata(output_path, total_samples)
    write_player_context_metadata(
        output_path,
        sample_count=total_samples,
        recent_horizon=PLAYER_CONTEXT_RECENT_HORIZON,
        future_horizon=PLAYER_CONTEXT_FUTURE_HORIZON,
    )
    print(f"wrote {total_samples} samples to {output_path} ({errors} files skipped)")
    return total_samples


def _append_file(source_path: Path, destination_handle: Any) -> None:
    if not source_path.exists():
        raise FileNotFoundError(f"missing shard artifact: {source_path}")
    with source_path.open("rb") as source:
        while True:
            chunk = source.read(1024 * 1024)
            if not chunk:
                break
            destination_handle.write(chunk)


def _count_nonblank_lines(path: Path) -> int:
    with path.open() as handle:
        return sum(1 for line in handle if line.strip())


def merge_preprocessed_shards(
    shard_paths: Iterable[str | Path],
    output_path: str | Path,
    *,
    expected_sample_count: int | None = None,
) -> int:
    resolved_shards = [Path(path) for path in shard_paths]
    if not resolved_shards:
        raise ValueError("at least one shard path is required")

    output_path = Path(output_path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    total_samples = 0
    recent_horizon: int | None = None
    future_horizon: int | None = None

    with output_path.open("wb") as data_out, open(group_ids_path(output_path), "wb") as groups_out, open(policy_value_requests_path(output_path), "wb") as requests_out, open(policy_value_player_context_path(output_path), "wb") as contexts_out:
        for shard_path in resolved_shards:
            shard_metadata = load_dataset_metadata(shard_path)
            shard_sample_count = int(shard_metadata["sample_count"])
            shard_context_metadata = load_player_context_metadata(shard_path)
            shard_recent_horizon = int(shard_context_metadata["recent_horizon"])
            shard_future_horizon = int(shard_context_metadata["future_horizon"])
            if recent_horizon is None:
                recent_horizon = shard_recent_horizon
                future_horizon = shard_future_horizon
            elif recent_horizon != shard_recent_horizon or future_horizon != shard_future_horizon:
                raise ValueError(
                    "player-context horizon mismatch across shards: "
                    f"expected ({recent_horizon}, {future_horizon}) got ({shard_recent_horizon}, {shard_future_horizon})"
                )

            _append_file(shard_path, data_out)
            _append_file(group_ids_path(shard_path), groups_out)
            _append_file(policy_value_requests_path(shard_path), requests_out)
            _append_file(policy_value_player_context_path(shard_path), contexts_out)

            request_count = _count_nonblank_lines(policy_value_requests_path(shard_path))
            context_count = _count_nonblank_lines(policy_value_player_context_path(shard_path))
            if request_count != shard_sample_count:
                raise ValueError(
                    f"policy/value request count mismatch for shard {shard_path}: expected {shard_sample_count}, got {request_count}"
                )
            if context_count != shard_sample_count:
                raise ValueError(
                    f"player-context count mismatch for shard {shard_path}: expected {shard_sample_count}, got {context_count}"
                )
            total_samples += shard_sample_count

    if expected_sample_count is not None and total_samples != expected_sample_count:
        raise ValueError(
            f"merged shard sample count mismatch: expected {expected_sample_count}, got {total_samples}"
        )

    write_dataset_metadata(output_path, total_samples)
    write_player_context_metadata(
        output_path,
        sample_count=total_samples,
        recent_horizon=recent_horizon or PLAYER_CONTEXT_RECENT_HORIZON,
        future_horizon=future_horizon or PLAYER_CONTEXT_FUTURE_HORIZON,
    )
    return total_samples


def preprocess_directory(
    input_dir: str | Path,
    output_path: str | Path,
    max_files: int | None = None,
    num_workers: int | None = None,
) -> int:
    """Convert all .ttrm files in a directory to a single binary training file.

    Binary format: contiguous array of f32 values, FLOATS_PER_SAMPLE per sample.
    Uses multiprocessing.Pool for parallel file processing.
    """
    input_dir = Path(input_dir)
    replay_shards = split_replay_files(input_dir, shard_count=1, max_files=max_files)
    replay_files = replay_shards[0] if replay_shards else []
    return preprocess_replay_files(replay_files, output_path, num_workers=num_workers)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Preprocess TTRM replays to binary training data")
    parser.add_argument("input_dir", help="Directory containing .ttrm files")
    parser.add_argument("output", help="Output .bin file path")
    parser.add_argument("--max-files", type=int, default=None, help="Max files to process")
    parser.add_argument("--workers", type=int, default=None, help="Number of parallel workers (default: all CPUs)")
    args = parser.parse_args()

    preprocess_directory(args.input_dir, args.output, args.max_files, args.workers)
