export type ReportStatus = "reported" | "skipped" | "failed";

export interface ReportOutcome {
    summonerName: string;
    championName: string;
    status: ReportStatus;
    message?: string | null;
}

export interface EndOfGameSummary {
    gameId: number;
    outcomes: ReportOutcome[];
}
