# The state file

- A finished work item's record in `history` was about 500 bytes in the pretty-printed state file, 572 with five-digit seconds in every phase and a 50-character title. Its `spend` (version 10) adds about 770 bytes at eight- to eleven-digit token counts and costs, so a record is about 1.2 KB and the cap of 100 keeps the records under about 120 KB, under about 140 KB with GitHub's longest title, 256 characters. They are written whole at every save and every beat.
