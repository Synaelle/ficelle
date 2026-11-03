// SPDX-License-Identifier: AGPL-3.0-or-later
// The shortest password the app takes for notes and backups. Dev and preview
// builds take any length, so testing does not mean typing long ones; release
// builds keep the real minimum.

export const MIN_PASSWORD = __APP_DEV_REPORTS__ ? 1 : 8
