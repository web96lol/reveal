export interface PlayerActionSummary {
  puuid: string;
  summonerId: number;
  gameName: string | null;
  tagLine: string | null;
  summonerName: string | null;
  friendRequestSent: boolean;
  reportSent: boolean;
}

export interface PostGameSummary {
  gameId: number;
  players: PlayerActionSummary[];
}
