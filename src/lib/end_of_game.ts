export type ReportOutcomeStatus =
  | "reported"
  | "skippedFriend"
  | "skippedSelf"
  | "failed";

export interface PlayerReportOutcome {
  summonerName: string;
  championName?: string | null;
  status: ReportOutcomeStatus;
  message: string;
}

export interface EndOfGameReport {
  gameId: number;
  results: PlayerReportOutcome[];
}
