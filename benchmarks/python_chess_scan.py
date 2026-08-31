#!/usr/bin/env python3
"""Comparable sequential scans for python-chess PGN APIs."""

import argparse
import logging
import os
import time
from typing import Tuple

import chess
import chess.pgn


class CountingVisitor(chess.pgn.BaseVisitor[Tuple[int, int]]):
    """Validate SAN and count moves without retaining a game tree."""

    def __init__(self) -> None:
        self.moves = 0
        self.errors = 0

    def visit_move(self, board: chess.Board, move: chess.Move) -> None:
        self.moves += 1

    def handle_error(self, error: Exception) -> None:
        self.errors += 1

    def result(self) -> Tuple[int, int]:
        return self.moves, self.errors


def scan(path: str, mode: str) -> Tuple[int, int, int]:
    games = 0
    moves = 0
    errors = 0
    with open(path, encoding="utf-8") as handle:
        if mode == "skip":
            while chess.pgn.skip_game(handle):
                games += 1
        elif mode == "visitor":
            while True:
                result = chess.pgn.read_game(handle, Visitor=CountingVisitor)
                if result is None:
                    break
                game_moves, game_errors = result
                games += 1
                moves += game_moves
                errors += game_errors
        else:
            while True:
                game = chess.pgn.read_game(handle)
                if game is None:
                    break
                games += 1
                errors += len(game.errors)
    return games, moves, errors


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("skip", "visitor", "model"))
    parser.add_argument("pgn")
    arguments = parser.parse_args()

    logging.getLogger("chess.pgn").setLevel(logging.CRITICAL)
    byte_count = os.path.getsize(arguments.pgn)
    started = time.perf_counter()
    games, moves, errors = scan(arguments.pgn, arguments.mode)
    elapsed = time.perf_counter() - started
    throughput = byte_count / elapsed / (1024 * 1024)

    print(f"python-chess: {chess.__version__}")
    print(f"mode: {arguments.mode}")
    print(f"bytes: {byte_count}")
    print(f"games: {games}")
    print(f"moves: {moves}")
    print(f"errors: {errors}")
    print(f"elapsed: {elapsed:.3f}s")
    print(f"throughput: {throughput:.2f} MiB/s")


if __name__ == "__main__":
    main()
