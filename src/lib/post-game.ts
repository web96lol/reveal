export interface PlayerActionSummary {
  puuid: string;
  summonerId: number;
  gameName: string | null;
  tagLine: string | null;
  summonerName: string | null;
  reportSent: boolean;
  categories: string[];
}

export interface PostGameSummary {
  gameId: number;
  players: PlayerActionSummary[];
}
