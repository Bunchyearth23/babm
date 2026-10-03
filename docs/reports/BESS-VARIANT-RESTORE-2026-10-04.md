# Restauration de la variante BESS — validation du 2026-10-04

Le parcours complet conserve la configuration Automation et importe une configuration BESS distincte. BABM garde la responsabilité de la fusion et des révisions. Cette validation couvre les archives et les exécutables compilés, sur des copies ; la sélection des configurations et l'écoute dans BeamNG restent à confirmer par l'utilisateur.

## Contrôles automatisés

- BABM : 25 tests release réussis, Clippy strict sur toutes les cibles et contrôle du formatage réussis.
- Cinq parcours compilés réussis : import standalone, import dans un pack, fusion initiale originale et plusieurs profils BESS, import avant fusion, et refus sans mutation d'un export dont un fichier original a été altéré.
- Les parcours couvrent aussi les révisions, le refus des imports répétés ou périmés, le routage configuration → moteur → deux émetteurs → WAV, la conservation des autres profils et la restauration par défusion.

Traces locales dans le dépôt BESS : `output/variant-restore-20261003/babm-final-clippy.log`, `babm-final-tests.log`, `babm-final-build.log` et `variant-roundtrip-final/verification.json`. Les logs BABM cités sont dans le même répertoire `output/variant-restore-20261003/`.

## Thunderhawk réel, sur copie

Le reçu `D:\Desktop\bess\output\variant-restore-20261003\real-thunderhawk\migration-verification.json` porte `passed: true`. La copie provenait d'un pack avec un ancien remplacement sonore BESS et un ancien add-on BESS déjà présents.

- 56 WAV Automation restaurés depuis la source préservée et vérifiée ; retrait du seul reçu de remplacement correspondant.
- 218 autres membres existants préservés, dont les 118 membres de l'ancien add-on et ses 112 WAV.
- Passage de deux à trois configurations : originale, ancien add-on BESS, nouveau profil BESS indépendant.
- Nouvelle configuration reliée à deux émetteurs et 112 WAV distincts ; nouvel import du même export refusé comme déjà installé.
- Sauvegarde intégrale du pack précédent vérifiée à l'identique ; archives originales préservées inchangées.
- Les quatre fichiers suivis dans l'installation réelle — pack, manifeste et deux archives préservées — ont conservé leurs empreintes. Cette vérification n'a pas modifié les mods installés.

| Élément vérifié | SHA-256 |
| --- | --- |
| BABM exécuté | `79ca4a34702567db31f7e7bc223f3b1275c12e085cde39affe08a54b8740ede5` |
| Export complet BESS importé | `10fd0555373621c2ec8575436334cf45e3dbc2797c183668d833811fa2ccdddf` |
| Source Automation préservée | `150bed820773f27447c79c2925326ed70d825725d9d796dc7f204f453cf0169f` |
| Copie du pack avant import et sauvegarde intégrale | `4daf11dd02a28dbd0edbdc6349b93cee0619f4a04041625721ebbe1748186323` |
| Copie du pack après import | `440299b7f6aebc77c22c7ae8935712756e12d18fc0ede3df19fd327a847aceef` |

La présence et le routage des fichiers ne prouvent ni leur sélection dans le jeu ni le rendu sonore perçu. W-001.4 et X-001 restent ouverts dans l'[index](../INDEX.md).
