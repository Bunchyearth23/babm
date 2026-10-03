# BABM — index de travail

Les identifiants sont stables. Les tests automatisés ne remplacent pas la validation dans BeamNG.

## Reprise

- Objectif : restaurer la configuration sonore BESS distincte, en laissant les fusions à BABM.
- Contrat : le ZIP complet contient l'original intact et les fichiers ajoutés de la variante. BABM importe uniquement ces composants, y compris dans un pack existant.
- Vérifié le 2026-10-04 : 25 tests BABM release, Clippy strict et formatage ; cinq parcours avec les exécutables compilés ; migration d'une copie du Thunderhawk réel, avec restauration de 56 WAV Automation, ajout d'une configuration BESS et conservation de l'ancien add-on. [Preuves et limites](reports/BESS-VARIANT-RESTORE-2026-10-04.md).
- À valider : sélection des configurations et écoute dans BeamNG. Les vérifications ont utilisé des copies ; les quatre fichiers installés suivis sont restés inchangés.

## Décisions

| ID | État | Décision |
| --- | --- | --- |
| D-001 | accepted | Un export `bess-variant-vehicle` ajoute une configuration BESS distincte. BESS ne fusionne pas les véhicules ; BABM remappe et regroupe les composants. |
| D-002 | accepted | Un profil conserve un `variant_id` stable. Réexporter vérifie puis remplace/retire uniquement ses fichiers enregistrés ; original et autres profils restent intacts. |
| D-003 | accepted | Les anciens exports `bess-full-vehicle` restent des remplacements WAV explicites, affichés séparément des imports de variante. |
| D-004 | accepted | Si un ancien remplacement est installé, l'import restaure les seuls WAV Automation possédés par son reçu vérifié puis ajoute la variante BESS. Source pristine manquante ou édition sonore externe : refus sans mutation. |

## Travaux

| ID | État | Chantier | Critère de sortie |
| --- | --- | --- | --- |
| W-001 | building | Import et fusion des ZIP complets originale + variante BESS | Tests de configurations, empreintes, réexports et restauration ; vérification d'un export BESS réel. |

### W-001

- [x] W-001.1 Contrat distinct, import des fichiers déclarés et registre par profil implémentés ; code : `src/bess/variants.rs`.
- [x] W-001.2 Fusion initiale et UI distinguent originale, profils BESS et anciens remplacements ; fixtures présentes dans `src/bess/variants.rs` et `src/app.rs`.
- [x] W-001.5 Migration d'un remplacement déjà installé vers originale + variante indépendante implémentée ; fixtures standalone, pack, autre source conservée et édition externe refusée.
- [x] W-001.3 Tests et export BESS réel vérifiés le 2026-10-04 : 25 tests release, contrôles stricts, cinq parcours compilés et migration Thunderhawk sur copie ; [preuve](reports/BESS-VARIANT-RESTORE-2026-10-04.md).
- [ ] W-001.4 Valider en jeu que les configurations originale et BESS apparaissent et utilisent chacune leurs sons.

## Risques

| ID | État | Priorité | Sujet | Condition de résolution |
| --- | --- | --- | --- | --- |
| X-001 | open | medium | La présence et le routage des fichiers ne prouvent pas le résultat dans BeamNG. | Essai utilisateur des deux configurations, puis réexport du même profil. |

Contrat détaillé : [BESS-BABM-EXCHANGE.md](BESS-BABM-EXCHANGE.md). Historique précédent : [rapport d'intégration](reports/BESS-BABM-INTEGRATION-2026-10-03.md).
