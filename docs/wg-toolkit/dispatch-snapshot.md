# Resource-derived dispatch snapshot

[Map home](README.md) · [Protocol](protocol.md) · [Regeneration commands](verification.md#reproduce-local-inspection)

Generated on 2026-11-11 using toolkit revision
`357a7f556a197079c21a371be5718e5211b41eda` and the supplied
`wot-eu-production-2.4.1.5498-windows-x64-sd-en` resources, with the
`dump_entity` example. This is **the toolkit's computed interpretation**, not an
independent dump from a running client. IDs may change with resource versions.

The registry includes modeled entities, dynamic component type slots, and static
components without independent type slots. The separate Account, Avatar and
Vehicle tables retain every reported property/method name, exposed index,
element ID and framing length. Long argument type trees are omitted here; run
the linked example to inspect complete signatures. Table presence does not imply
that the emulator implements the associated gameplay feature.

NRL component IDs use a separately filtered sequence; registry type slots below
must not automatically be used as NRL IDs.

## Full modeled registry

```text
== entities ==
    1  Account
    2  Avatar
    3  ArenaInfo
    4  ClientSelectableObject
    5  HangarVehicle
    6  Vehicle
    7  AreaDestructibles
    8  OfflineEntity
    9  Flock
   10  FlockExotic
   11  Login
   12  DetachedTurret
   13  DebugDrawEntity
   14  ClientSelectableCameraObject
   15  ClientSelectableCameraVehicle
   16  ClientSelectableWebLinksOpener
   17  ClientSelectableEasterEgg
   18  EmptyEntity
   19  LimitedVisibilityEntity
   20  HeroTank
   21  PlatoonTank
   22  PlatoonLighting
   23  SectorBase
   24  Sector
   25  DestructibleEntity
   26  StepRepairPoint
   27  ProtectionZone
   28  TeamInfo
   29  AvatarInfo
   30  ArenaObserverInfo
   31  AreaOfEffect
   32  AttackBomber
   33  AttackArtilleryFort
   34  PersonalDeathZone
   35  ClientSelectableRankedObject
   36  SimulatedVehicle
   37  ClientSelectableHangarsSwitcher
   38  StaticDeathZone
   39  BasicMine
   40  NetworkEntity
   41  Mine
   42  Loot
   43  Placement
   44  InfluenceZone
   45  BattleRoyaleRadio
   46  ThunderStrike
   47  Comp7Lighting
   48  ApplicationPoint
   49  HWPersonalDeathZone
   50  HWDeathZone
   51  ReplayAccount
   52  SPGZone
== dynamic components ==
   53  OwnVehicle
   54  InAoEZone
   55  DogTagComponent
   56  DogTagsInfo
   57  AutocannonsShootingCounter
   58  AutocannonProjectileRelaxationController
   59  LabelComponent
   60  GunMarkerComponent
   61  ExtendedSPG
   62  AtGunpoint
   63  EnemyShotPredictorController
   64  MapsTrainingStorageComponent
   65  BattleHintsComponent
   66  Fire
   67  RocketAccelerationController
   68  RechargeableNitroController
   69  KillCamDataComponent
   70  AutoShootGunController
   71  TwinGunController
   72  SecondaryGunComponent
   73  RocketAcceleration
   74  TemperatureGunController
   75  OverheatGunComponent
   76  HeatingZonesGunComponent
   77  VisualScriptEquipment
   78  VisualScriptCache
   79  VisualScriptOverrides
   80  VisualScriptEquipmentPublic
   81  VisualScriptPlan
   82  LiveTagsInfoComponent
   83  ArenaCommendationsMessageController
   84  AvatarCommendations
   85  VehicleCircle
   86  VehicleRadarDetection
   87  IlluminationFlareDetection
   88  IlluminationFlareTargetController
   89  VehicleBuff
   90  IncomingDamageModifier
   91  GunReloadBoost
   92  NextClipsReloader
   93  EnemyVehiclesController
   94  VehicleSkillCounter
   95  Autodestroy
   96  VehicleFamiliar
   97  Proximity
   98  Capturable
   99  CaptureControl
  100  PoiCaptureBlockerComponent
  101  PoiComponent
  102  PoiVehicleComponent
  103  DynComponentsGroup
  104  DynComponentsGroupHelper
  105  LootableComponent
  106  DynObstacleComponent
  107  ImpassableZoneComponent
  108  SpamProtectionComponent
  109  TestReplicableComponent
  110  ShotsReceiver
  111  DeathComponent
  112  HealthComponent
  113  VehicleRespawnComponent
  114  VehicleLivesComponent
  115  TeamInfoLivesComponent
  116  TeamInfoPBHComponent
  117  PveHudWidgetsStateComponent
  118  PveMinimapController
  119  TargetMarkersComponent
  120  EntityMarkerComponent
  121  EntityAreaBorderComponent
  122  EntityPolygonalTriggerComponent
  123  StaticDeathZoneVisual
  124  EntityVisualScriptRunner
  125  SequenceNetworkSync
  126  StateMachineNetworkSync
  127  DualAccuracy
  128  ArenaVehiclesInfo
  129  AvatarVehiclesInfo
  130  ForcedReloadComponent
  131  FairplayVehicleBattleStatsComponent
  132  AvatarFairplayMessageComponent
  133  NetworkReplicationPointComponent
  134  NetworkVehicleHierarchy
  135  ConcentrationModeComponent
  136  OverheatStacksController
  137  ImprovedRammingController
  138  PowerModeController
  139  ExtraShotClipComponent
  140  BattleFuryController
  141  AccuracyStacksController
  142  SupportWeaponComponent
  143  PillboxSiegeComponent
  144  StationaryReloadController
  145  LowChargeShotController
  146  LowChargeShotPublicController
  147  ChargeShotComponent
  148  TargetDesignatorController
  149  TargetDesignatorTargetController
  150  TargetDesignatorTeamController
  151  StanceDanceController
  152  AutoreloaderSurgeController
  153  CrestMovingController
  154  ChargeableBurstComponent
  155  DebuffController
  156  StagedJetBoostersController
  157  PropellantGunController
  158  WheeledDashController
  159  SightPointerComponent
  160  AuxiliaryRocketLauncherComponent
  161  ShellParamsSwitcherController
  162  ShellCalibrationController
  163  BustleFeedController
  164  SpecBoostModeComponent
  165  ArenaLootComponent
  166  ArenaMinesComponent
  167  ArenaDeathZonesComponent
  168  ArenaEntityPositionGridComponent
  169  ArenaInfoDeathZonesComponent
  170  VehicleDeathZoneEffect
  171  SpawnKeyPointController
  172  SpawnKeyPointTeamInfo
  173  BattleXP
  174  ConeVisibility
  175  Radar
  176  InBattleUpgradeReadiness
  177  InBattleUpgrades
  178  VehicleLoot
  179  InBattleUpgradesAvatar
  180  AvatarBattleRoyaleComponent
  181  BattleXPArenaInfo
  182  LootArenaInfo
  183  VehicleCorrodingShotComponent
  184  VehicleCorrodingShotPreparingComponent
  185  VehicleAdaptationHealthRestoreComponent
  186  VehicleThunderStrikeComponent
  187  VehicleShotPassionComponent
  188  VehicleSelfBuffComponent
  189  VehicleBerserkerComponent
  190  ArenaInfoBRComponent
  191  VehicleHealPointComponent
  192  VehicleHealingComponent
  193  VehicleFireCircleEffectComponent
  194  VehicleBRRespawnComponent
  195  VehicleBRRespawnEffectComponent
  196  BattleRoyaleObserverInfoComponent
  197  BattleRoyaleCheatsComponent
  198  VehicleBRStPatrickComponent
  199  VehicleComp7Component
  200  ArenaInfoComp7Component
  201  TeamInfoComp7Component
  202  ArenaObserverInfoComp7Component
  203  AvatarComp7BaseComponent
  204  VehiclePrestigePoint
  205  VehicleRoleSkill
  206  VehicleInspireController
  207  VehicleHealController
  208  VehicleComp7LightComponent
  209  ArenaInfoComp7LightComponent
  210  IndividualRespawnTimerComponent
  211  RespawnFrameworkVehicleComponent
  212  DisconnectRespawnValidatorComponent
  213  RespawnPolicyVehicleLinkComponent
  214  FortRushScoreComponent
  215  FortRushDevelopmentHelper
  216  CapturePointComponent
  217  FortRushVehicleRespawnComponent
  218  FortRushVehicleBattleFeedbackComponent
  219  CapturePointInvaderComponent
  220  FortRushHighlighter
  221  VehicleExpirableComponent
  222  FortRushDamageResistanceComponent
  223  FortRushCapturePointStateMachineComponent
  224  FortRushFairplayVehicleBattleStatsComponent
  225  FortRushVehicleStatsCollector
  226  FortRushSoundPlayerComponent
  227  FLAvatarComponent
  228  FLReservesComponent
  229  FLAvatarReservesComponent
  230  FLBattleUpgradeReserves
  231  FLVehicleComponent
  232  FLArenaMinefieldComponent
  233  FLVehicleRegenerationKitComponent
  234  FLStealthRadarComponent
  235  FLArenaBattleModifiersComponent
  236  HWTeamInfoStatsComponent
  237  HWVehicleBattleStatsComponent
  238  HWArenaDropComponent
  239  HWArenaPhasesComponent
  240  HWVehicleSoulsContainerComponent
  241  HWVehicleCheatsComponent
  242  HWBuffNitroComponent
  243  HWBuffDynamicFactorsComponent
  244  HWBuffFactorsComponent
  245  HWBuffSequencesComponent
  246  HWBuffPrefabComponent
  247  HWBuffPanelComponent
  248  HWSoulsCollectorComponent
  249  HWArenaVehicleKillComponent
  250  HWBuffRepairComponent
  251  HWBuffPeriodicHealComponent
  252  HWVehicleAIComponent
  253  HWBuffRadiusAoEComponent
  254  HWVehicleFireComponent
  255  HWBuffIgniteVehicleComponent
  256  HWBuffDrainHealthComponent
  257  HWBuffDamageVehicleComponent
  258  HWBuffVehicleIconComponent
  259  HWBuffVehicleModifiersComponent
  260  HWVehicleRepairComponent
  261  HWVehicleFeedbackComponent
  262  HWVehicleDissolveComponent
  263  HWBuffDamageResistsComponent
  264  HWBuffSoulsDrainResistComponent
  265  HWArenaVehicleRemovalComponent
  266  HWBuffInterruptibleComponent
  267  HWBuffCircularVehicleProximityComponent
  268  HWBuffBossAuraComponent
  269  HWBuffDrainSoulsComponent
  270  HWBuffDrainLifeComponent
  271  HWBuffDrainMarkerComponent
  272  HWBuffEffectsListPlayerComponent
  273  HWBuffFlashBangEffectComponent
  274  HWVehicleMaxHealthModifierComponent
  275  HWVehicleModifiersComponent
  276  HWVehicleDetector
  277  HWVehicleFollowedSpawner
  278  HWBuffAddBuffOnShotComponent
  279  HWBuffAddMaxHealthComponent
  280  HWBuffInfinitePeriodicHealComponent
  281  HWArenaInfoBossHealthBarComponent
  282  HWBuffVehicleEffectsComponent
  283  HWBuffLaserSightComponent
  284  HWArenaSoundComponent
  285  HWVehicleSoundComponent
  286  HWVehicleDeathOnFall
  287  HWBuffInvisibilityForAIComponent
  288  HWBuffSoundComponent
  289  HWKafkaVehicleLoggingComponent
  290  HWKafkaLoggingComponent
  291  HWBuffDissolveComponent
  292  HWBuffAddDynGroupComponent
  293  HWLootVisualComponent
  294  HWLootVehicleWatcherComponent
  295  HWArenaPhaseProgressionComponent
  296  HWNitroAccelerationControl
  297  HWFairplayVehicleBattleStatsComponent
  298  HWBuffShootComponent
  299  HWVehicleInstantShotComponent
  300  HWVehicleShotChargerComponent
  301  HWBuffStunVehicleComponent
  302  HWVehicleMultiAuraHandlerComponent
  303  HWVehicleHitDirectionComponent
  304  HWVehicleSpawnInfoComponent
  305  HWArenaSpawnInfoComponent
  306  HWVehicleOptionsRequesterComponent
  307  HWLootableComponent
  308  HWVehicleAnomaliesComponent
  309  HWVehicleStatusNotificationComponent
  310  HWEntityChaserComponent
  311  HWBuffDamageVariableComponent
  312  HWBuffInstantKillComponent
  313  HWBuffVehicleKillsCounterComponent
  314  ArenaAchievementsComponent
  315  VehicleAchievementsComponent
  316  LSTeamInfoStatsComponent
  317  LSTeamInfoVoiceChatComponent
  318  LSVehicleBattleStatsComponent
  319  LSArenaDropComponent
  320  LSArenaPhasesComponent
  321  LSVehicleSoulsContainerComponent
  322  LSVehicleCheatsComponent
  323  LSBuffNitroComponent
  324  LSBuffNitroAccelerationComponent
  325  LSBuffFactorsComponent
  326  LSBuffSequencesComponent
  327  LSBuffWithBoosterSequencesComponent
  328  LSArenaVehicleKillComponent
  329  LSBuffRepairComponent
  330  LSBuffPeriodicHealComponent
  331  LSVehicleAIComponent
  332  LSBuffRadiusAoEComponent
  333  LSVehicleFireComponent
  334  LSBuffIgniteVehicleComponent
  335  LSBuffDrainHealthComponent
  336  LSBuffDamageVehicleComponent
  337  LSBuffVehicleIconComponent
  338  LSVehicleRepairComponent
  339  LSVehicleFeedbackComponent
  340  LSVehicleDissolveComponent
  341  LSBuffDamageResistsComponent
  342  LSBuffSoulsDrainResistComponent
  343  LSArenaVehicleRemovalComponent
  344  LSArenaWatersComponent
  345  LSBuffInterruptibleComponent
  346  LSBuffEffectsListPlayerComponent
  347  LSVehicleMaxHealthModifierComponent
  348  LSBuffAddBuffOnShotComponent
  349  LSBuffAddMaxHealthComponent
  350  LSBuffVehicleEffectsComponent
  351  LSBuffLaserSightComponent
  352  LSArenaSoundComponent
  353  LSVehicleSoundComponent
  354  LSVehicleDeathOnFall
  355  LSBuffInvisibilityForAIComponent
  356  LSBuffSoundComponent
  357  LSKafkaVehicleLoggingComponent
  358  LSKafkaLoggingComponent
  359  LSBuffDissolveComponent
  360  LSBuffAddDynGroupComponent
  361  LSLootVisualComponent
  362  LSLootVehicleWatcherComponent
  363  LSNitroAccelerationControl
  364  LSFairplayVehicleBattleStatsComponent
  365  LSBuffShootComponent
  366  LSVehicleInstantShotComponent
  367  LSVehicleShotChargerComponent
  368  LSBuffStunVehicleComponent
  369  LSVehicleMultiAuraHandlerComponent
  370  LSVehicleShellsComponent
  371  LSWaveProgressComponent
  372  LSVehicleHitDirectionComponent
  373  LSBuffOnShotComponent
  374  LSVehicleInvulnerableMarkerComponent
  375  LSBuffAddDynGroupOnEventComponent
  376  LSVehicleBoosterFactorsComponent
  377  LSObeliskInfoComponent
  378  LSBeamTargetComponent
  379  LSBuffNotificationComponent
  380  LSVehicleContinuousTurretRotator
  381  LSVehicleDrownedComponent
  382  StoryModeAfkController
  383  StoryModeDamageResistsComponent
  384  StoryModeAccuracyOverrideComponent
  385  StoryModeModulesImmunityComponent
  386  StoryModeLootableComponent
  387  SMReconAbilityEntityComponent
  388  SMReconAbilityVehicleComponent
  389  SMDistractionAbilityEntityComponent
  390  SMDistractionAbilityArenaInfoComponent
  391  SMDetectionDelayObserverComponent
  392  SMDetectionDelayObservableComponent
  393  SMVehicleRespawnComponent
  394  SMAbilitiesTrackerComponent
  395  SMAbilitiesInterruptComponent
  396  SMAbilitiesRechargeComponent
  397  SMArtilleryStrikeChannelComponent
  398  SMRemovableDynGroupComponent
  399  SMSequencesComponent
  400  SMSound3DComponent
  401  SMSound3DObjectComponent
  402  BunkerLogicComponent
== static components (no type id) ==
       PoiTeamInfoComponent
       ArenaInBattleVehicleSwitch
       AvatarInBattleVehicleSwitch
       VehicleInBattleSwitch
       TeamInfoInBattleVehicleSwitch
       ArenaTriggerSystemComponent
       ArenaVehiclesInfoSyncComponent
       ArenaFairplayComponent
       ArenaFairplayKafkaLoggingComponent
       AccountFairplayComponent
       ArenaCommendationsMasterComponent
       ArenaDynObstaclesComponent
       ArenaFogOfWarComponent
       ArenaRiggedBattleComponent
       ArenaPBHComponent
       AccountPBHComponent
       BattleRoyaleComponent
       AccountBattleRoyaleComponent
       AccountBattleRoyaleTournamentComponent
       ArenaComp7Component
       AccountComp7Component
       PrebattleComp7Component
       AccountComp7LightComponent
       ArenaComp7LightComponent
       KafkaLoggingComponent
       VehicleHealthBroadcastComponent
       VehicleVisionOverrideComponent
       RespawnFrameworkArenaComponent
       FortRushComponent
       FortRushAccountComponent
       FLArenaComponent
       FLAccountComponent
       FLAccountPrebattleComponent
       FLAccountBattleResultComponent
       FLArenaBattleResultComponent
       FLRespawnComponent
       AccountFunRandomComponent
       FunRandomKafkaLoggingComponent
       FunRandomArenaController
       HalloweenAccountComponent
       HalloweenComponent
       HWArenaBattleResultComponent
       HWKafkaPublisherComponent
       InBattleAchievementsComponent
       LaPingerComponent
       LastStandComponent
       LSAccountComponent
       LSAccountEquipmentController
       LSArenaBattleResultComponent
       LSKafkaPublisherComponent
       OpenBundleAccountComponent
       ResourceWellAccountComponent
       ServerReplayArenaComponent
       StoryModeMissions
       StoryModeAccountComponent
       StoryModeArenaComponent
       StoryModeAvatarComponent
```

## Account


### Account (type id 1): 4 properties

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0xA7 | incarnationID | Fixed(8) |
| 1 | 0xA8 | requiredVersion_2410 | Variable8 |
| 2 | 0xA9 | name | Variable8 |
| 3 | 0xAA | initialServerSettings | Variable8 |

### client methods: 44

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x4E | onArenaCreated | Fixed(0) |
| 1 | 0x4F | onPrebattleLeft | Fixed(0) |
| 2 | 0x50 | reloadShop | Fixed(0) |
| 3 | 0x51 | onEnqueued | Fixed(1) |
| 4 | 0x52 | onDequeued | Fixed(1) |
| 5 | 0x53 | onKickedFromQueue | Fixed(1) |
| 6 | 0x54 | onPrebattleJoinFailure | Fixed(1) |
| 7 | 0x55 | onKickedFromArena | Fixed(1) |
| 8 | 0x56 | onKickedFromPrebattle | Fixed(1) |
| 9 | 0x57 | onCenterIsLongDisconnected | Fixed(1) |
| 10 | 0x58 | resyncDossiers | Fixed(1) |
| 11 | 0x59 | onPrebattleJoined | Fixed(4) |
| 12 | 0x5A | onUnitCallOk | Fixed(4) |
| 13 | 0x5B | receiveServerStats | Fixed(8) |
| 14 | 0x5C | onChatAction | Variable8 |
| 15 | 0x5D | messenger_onActionByServer_chat2 | Variable8 |
| 16 | 0x5E | onCmdResponse | Variable8 |
| 17 | 0x5F | onCmdResponseExt | Variable8 |
| 18 | 0x60 | onTokenReceived | Variable8 |
| 19 | 0x61 | processInvitations | Variable8 |
| 20 | 0x62 | onKickedFromServer | Variable8 |
| 21 | 0x63 | onEnqueueFailure | Variable8 |
| 22 | 0x64 | onIGRTypeChanged | Variable8 |
| 23 | 0x65 | onArenaJoinFailure | Variable8 |
| 24 | 0x66 | receiveActiveArenas | Variable8 |
| 25 | 0x67 | receiveQueueInfo | Variable8 |
| 26 | 0x68 | updatePrebattle | Variable8 |
| 27 | 0x69 | update | Variable8 |
| 28 | 0x6A | onUnitUpdate | Variable8 |
| 29 | 0x6B | onUnitNotify | Variable8 |
| 30 | 0x6C | onUnitError | Variable8 |
| 31 | 0x6D | onUnitBrowserError | Variable8 |
| 32 | 0x6E | onUnitBrowserResultsSet | Variable8 |
| 33 | 0x6F | onUnitBrowserResultsUpdate | Variable8 |
| 34 | 0x70 | onGlobalMapUpdate | Variable8 |
| 35 | 0x71 | onGlobalMapReply | Variable8 |
| 36 | 0x72 | onSendPrebattleInvites | Variable8 |
| 37 | 0x73 | onClanInfoReceived | Variable8 |
| 38 | 0x74 | receiveNotification | Variable8 |
| 39 | 0x75 | receiveConversionResults | Variable8 |
| 40 | 0x76 | showGUI | Variable16 |
| 41 | 0x77 | setTournamentToken | Variable8 |
| 42 | 0x78 | setParticipants | Variable16 |
| 43 | 0x79 | pingMeAndThenJustTouchMe | Variable8 |

### cell methods: 0

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |

### base methods: 40

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x88 | accountUnitBrowser_unsubscribe | Fixed(0) |
| 1 | 0x89 | onStreamComplete | Fixed(3) |
| 2 | 0x8A | requestToken | Fixed(3) |
| 3 | 0x8B | accountUnitBrowser_subscribe | Fixed(3) |
| 4 | 0x8C | doCmdNoArgs | Fixed(4) |
| 5 | 0x8D | accountUnitBrowser_doCmd | Fixed(4) |
| 6 | 0x8E | accountUnitBrowser_recenter | Fixed(7) |
| 7 | 0x8F | doCmdInt | Fixed(12) |
| 8 | 0x90 | makeDenunciation | Fixed(13) |
| 9 | 0x91 | accountUnitClient_join | Fixed(16) |
| 10 | 0x92 | logStreamCorruption | Fixed(18) |
| 11 | 0x93 | doCmdInt2 | Fixed(20) |
| 12 | 0x94 | accountDebugger_registerDebugTaskResult | Fixed(20) |
| 13 | 0x95 | doCmdInt3 | Fixed(28) |
| 14 | 0x96 | ackCommand | Fixed(33) |
| 15 | 0x97 | doCmdInt4 | Fixed(36) |
| 16 | 0x98 | chatCommandFromClient | Variable8 |
| 17 | 0x99 | inviteCommand | Variable8 |
| 18 | 0x9A | messenger_onActionByClient_chat2 | Variable8 |
| 19 | 0x9B | doCmdStr | Variable8 |
| 20 | 0x9C | doCmdInt2Str | Variable8 |
| 21 | 0x9D | doCmdInt3Str | Variable8 |
| 22 | 0x9E | doCmdIntArr | Variable8 |
| 23 | 0x9F | doCmdIntStr | Variable8 |
| 24 | 0xA0 | doCmdIntStrArr | Variable8 |
| 25 | 0xA1 | doCmdIntArrStrArr | Variable8 |
| 26 | 0xA2 | doCmdStrArr | Variable8 |
| 27 | 0xA3 | accountAvatar_sendAccountStats | Variable8 |
| 28 | 0xA4 | accountPrebattle_createTraining | Variable8 |
| 29 | 0xA5 | accountPrebattle_createDevPrebattle | Variable8 |
| 30 | 0xA6 | accountPrebattle_sendPrebattleInvites | Variable8 |
| 31 | 0xA7 | accountGlobalMapConnector_callGlobalMapMethod | Variable8 |
| 32 | 0xA8 | accountUnitClient_create | Variable8 |
| 33 | 0xA9 | accountUnitClient_doCmd | Variable8 |
| 34 | 0xAA | accountUnitClient_sendInvites | Variable8 |
| 35 | 0xAB | accountUnitClient_setRosterSlots | Variable8 |
| 36 | 0xAC | accountDebugger_sendDebugTaskResultChunk | Variable8 |
| 37 | 0xAD | banUnbanUser | Variable8 |
| 38 | 0xAE | setKickAtTime | Variable8 |
| 39 | 0xAF | setDevelopmentFeature | Variable8 |

## Avatar


### Avatar (type id 2): 34 properties

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0xA7 | isObserverFPV | Fixed(1) |
| 1 | 0xA8 | numOfObservers | Fixed(1) |
| 2 | 0xA9 | arenaBonusType | Fixed(1) |
| 3 | 0xAA | arenaGuiType | Fixed(1) |
| 4 | 0xAB | weatherPresetID | Fixed(1) |
| 5 | 0xAC | tkillIsSuspected | Fixed(1) |
| 6 | 0xAD | team | Fixed(1) |
| 7 | 0xAE | isObserverBothTeams | Fixed(1) |
| 8 | 0xAF | observableTeamID | Fixed(1) |
| 9 | 0xB0 | isGunLocked | Fixed(1) |
| 10 | 0xB1 | ownVehicleGear | Fixed(1) |
| 11 | 0xB2 | customizationDisplayType | Fixed(1) |
| 12 | 0xB3 | shouldSendKillcamSimulationData | Fixed(1) |
| 13 | 0xB4 | denunciationsLeft | Fixed(2) |
| 14 | 0xB5 | ownVehicleHullAimingPitchPacked | Fixed(2) |
| 15 | 0xB6 | battleChatRestriction | Fixed(2) |
| 16 | 0xB7 | arenaTypeID | Fixed(4) |
| 17 | 0xB8 | playerVehicleID | Fixed(4) |
| 18 | 0xB9 | arenaUniqueID | Fixed(8) |
| 19 | 0xBA | ownVehicleAuxPhysicsData | Fixed(8) |
| 20 | 0xBB | playLimits | Fixed(16) |
| 21 | 0xBC | remoteCamera | Fixed(22) |
| 22 | 0xBD | name | Variable8 |
| 23 | 0xBE | sessionID | Variable8 |
| 24 | 0xBF | arenaExtraData | Variable8 |
| 25 | 0xC0 | clientCtx | Variable8 |
| 26 | 0xC1 | ammoViews | Variable8 |
| 27 | 0xC2 | goodiesSnapshot | Variable8 |
| 28 | 0xC3 | isVehicleConfirmed | Fixed(1) |
| 29 | 0xC4 | vehicleSpawnList | Variable8 |
| 30 | 0xC5 | spawnInfoForVehicle | Variable8 |
| 31 | 0xC6 | spawnPoints | Variable8 |
| 32 | 0xC7 | isPositionValid | Fixed(1) |
| 33 | 0xC8 | wrongApplicationPoint | Fixed(12) |

### client methods: 70

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x4E | notifyCannotStartRecovering | Fixed(0) |
| 1 | 0x4F | notifyCancelled | Fixed(0) |
| 2 | 0x50 | enteringProtectionZone | Fixed(1) |
| 3 | 0x51 | leavingProtectionZone | Fixed(1) |
| 4 | 0x52 | protectionZoneShooting | Fixed(1) |
| 5 | 0x53 | onSectorShooting | Fixed(1) |
| 6 | 0x54 | onRankUpdate | Fixed(1) |
| 7 | 0x55 | onAutoAimVehicleLost | Fixed(1) |
| 8 | 0x56 | onKickedFromArena | Fixed(1) |
| 9 | 0x57 | onXPUpdated | Fixed(2) |
| 10 | 0x58 | onRoundFinished | Fixed(2) |
| 11 | 0x59 | removeVehicle | Fixed(4) |
| 12 | 0x5A | updateTargetVehicleID | Fixed(4) |
| 13 | 0x5B | onDestructibleDestroyed | Fixed(5) |
| 14 | 0x5C | updateResourceAmount | Fixed(5) |
| 15 | 0x5D | updateVehicleQuickShellChanger | Fixed(5) |
| 16 | 0x5E | onSectorBaseAction | Fixed(6) |
| 17 | 0x5F | onRepairPointAction | Fixed(6) |
| 18 | 0x60 | updateVehicleHealth | Fixed(9) |
| 19 | 0x61 | updateVehicleSetting | Fixed(9) |
| 20 | 0x62 | onStepRepairPointAction | Fixed(11) |
| 21 | 0x63 | onVehicleHealthChanged | Fixed(12) |
| 22 | 0x64 | welcomeToSector | Fixed(12) |
| 23 | 0x65 | enemySPGHit | Fixed(12) |
| 24 | 0x66 | updateState | Fixed(13) |
| 25 | 0x67 | onCrewRoleFactorAndRankUpdate | Fixed(13) |
| 26 | 0x68 | onCombatEquipmentShotLaunched | Fixed(14) |
| 27 | 0x69 | onSwitchViewpoint | Fixed(16) |
| 28 | 0x6A | stopTracer | Fixed(16) |
| 29 | 0x6B | onCollisionWithVehicle | Fixed(16) |
| 30 | 0x6C | onFrictionWithVehicle | Fixed(17) |
| 31 | 0x6D | updateVehicleAmmo | Fixed(18) |
| 32 | 0x6E | showOwnVehicleHitDirection | Fixed(21) |
| 33 | 0x6F | enemySPGShotSound | Fixed(24) |
| 34 | 0x70 | showHittingArea | Fixed(34) |
| 35 | 0x71 | showCarpetBombing | Fixed(34) |
| 36 | 0x72 | updateTargetingInfo | Fixed(36) |
| 37 | 0x73 | showTracer | Fixed(50) |
| 38 | 0x74 | onChatAction | Variable8 |
| 39 | 0x75 | messenger_onActionByServer_chat2 | Variable8 |
| 40 | 0x76 | onCmdResponse | Variable8 |
| 41 | 0x77 | onCmdResponseExt | Variable8 |
| 42 | 0x78 | processInvitations | Variable8 |
| 43 | 0x79 | onTokenReceived | Variable8 |
| 44 | 0x7A | updateTeamsHealthPercentage | Variable8 |
| 45 | 0x7B | externalTrigger | Variable8 |
| 46 | 0x7C | syncPurchasedAbilities | Variable8 |
| 47 | 0x7D | onRandomReserveOffer | Variable8 |
| 48 | 0x7E | showDestructibleShotResults | Variable8 |
| 49 | 0x7F | update | Variable8 |
| 50 | 0x80 | onKickedFromServer | Variable8 |
| 51 | 0x81 | onIGRTypeChanged | Variable8 |
| 52 | 0x82 | receiveAccountStats | Variable8 |
| 53 | 0x83 | showOtherVehicleDamagedDevices | Variable8 |
| 54 | 0x84 | showShotResults | Variable8 |
| 55 | 0x85 | showDevelopmentInfo | Variable8 |
| 56 | 0x86 | onBattleEvents | Variable8 |
| 57 | 0x87 | battleEventsSummary | Variable8 |
| 58 | 0x88 | updateArena | Variable8 |
| 59 | 0x89 | receivePhysicsDebugInfo | Variable8 |
| 60 | 0x8A | updateCarriedFlagPositions | Variable8 |
| 61 | 0x8B | receiveNotification | Variable8 |
| 62 | 0x8C | updateAvatarPrivateStats | Variable8 |
| 63 | 0x8D | onSmoke | Variable8 |
| 64 | 0x8E | handleScriptEventFromServer | Variable8 |
| 65 | 0x8F | onRandomEvent | Variable8 |
| 66 | 0x90 | updateSpawnList | Variable16 |
| 67 | 0x91 | explodeProjectile | Variable16 |
| 68 | 0x92 | updateQuestProgress | Variable16 |
| 69 | 0x93 | setUpdatedGoodiesSnapshot | Variable16 |

### cell methods: 24

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x10 | vehicle_shoot | Fixed(0) |
| 1 | 0x11 | setDualGunCharger | Fixed(1) |
| 2 | 0x12 | vehicle_moveWith | Fixed(1) |
| 3 | 0x13 | setServerMarker | Fixed(1) |
| 4 | 0x14 | setSendKillCamSimulationData | Fixed(1) |
| 5 | 0x15 | submitPlayerSatisfactionRating | Fixed(1) |
| 6 | 0x16 | bindToVehicle | Fixed(4) |
| 7 | 0x17 | monitorVehicleDamagedDevices | Fixed(4) |
| 8 | 0x18 | activateEquipment | Fixed(4) |
| 9 | 0x19 | switchObserverFPV | Fixed(5) |
| 10 | 0x1A | autoAim | Fixed(5) |
| 11 | 0x1B | switchViewPointOrBindToVehicle | Fixed(5) |
| 12 | 0x1C | vehicle_changeSetting | Fixed(5) |
| 13 | 0x1D | vehicle_stopTrackingWithGun | Fixed(8) |
| 14 | 0x1E | setupAmmo | Fixed(8) |
| 15 | 0x1F | moveTo | Fixed(12) |
| 16 | 0x20 | vehicle_trackWorldPointWithGun | Fixed(12) |
| 17 | 0x21 | vehicle_trackRelativePointWithGun | Fixed(12) |
| 18 | 0x22 | setEquipmentApplicationPoint | Fixed(22) |
| 19 | 0x23 | reportClientStats | Fixed(24) |
| 20 | 0x24 | confirmVehicleSelection | Fixed(0) |
| 21 | 0x25 | switchSetup | Fixed(6) |
| 22 | 0x26 | chooseVehicle | Fixed(8) |
| 23 | 0x27 | checkPositionForEquipment | Fixed(16) |

### base methods: 35

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x88 | logLag | Fixed(0) |
| 1 | 0x89 | setClientReady | Fixed(0) |
| 2 | 0x8A | leaveArena | Fixed(0) |
| 3 | 0x8B | onLoginToCellFailed | Fixed(0) |
| 4 | 0x8C | confirmBattleResultsReceiving | Fixed(0) |
| 5 | 0x8D | vehicle_replenishAmmo | Fixed(0) |
| 6 | 0x8E | enableFrontLineDevInfo | Fixed(1) |
| 7 | 0x8F | onStreamComplete | Fixed(3) |
| 8 | 0x90 | requestToken | Fixed(3) |
| 9 | 0x91 | doCmdNoArgs | Fixed(4) |
| 10 | 0x92 | makeDenunciation | Fixed(9) |
| 11 | 0x93 | doCmdInt | Fixed(12) |
| 12 | 0x94 | vehicle_teleport | Fixed(16) |
| 13 | 0x95 | logStreamCorruption | Fixed(18) |
| 14 | 0x96 | doCmdInt2 | Fixed(20) |
| 15 | 0x97 | doCmdInt3 | Fixed(28) |
| 16 | 0x98 | ackCommand | Fixed(33) |
| 17 | 0x99 | receiveFakeShot | Fixed(33) |
| 18 | 0x9A | doCmdInt4 | Fixed(36) |
| 19 | 0x9B | chatCommandFromClient | Variable8 |
| 20 | 0x9C | inviteCommand | Variable8 |
| 21 | 0x9D | messenger_onActionByClient_chat2 | Variable8 |
| 22 | 0x9E | doCmdStr | Variable8 |
| 23 | 0x9F | doCmdInt2Str | Variable8 |
| 24 | 0xA0 | doCmdInt3Str | Variable8 |
| 25 | 0xA1 | doCmdIntArr | Variable8 |
| 26 | 0xA2 | doCmdIntStr | Variable8 |
| 27 | 0xA3 | doCmdIntStrArr | Variable8 |
| 28 | 0xA4 | doCmdIntArrStrArr | Variable8 |
| 29 | 0xA5 | doCmdStrArr | Variable8 |
| 30 | 0xA6 | banUnbanUser | Variable8 |
| 31 | 0xA7 | sendAccountStats | Variable8 |
| 32 | 0xA8 | setClientCtx | Variable8 |
| 33 | 0xA9 | setDevelopmentFeature | Variable8 |
| 34 | 0xAA | addBotToArena | Variable8 |

## Vehicle


### Vehicle (type id 6): 50 properties

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0xA7 | burnoutLevel | Fixed(1) |
| 1 | 0xA8 | isStrafing | Fixed(1) |
| 2 | 0xA9 | isHidden | Fixed(1) |
| 3 | 0xAA | physicsMode | Fixed(1) |
| 4 | 0xAB | siegeState | Fixed(1) |
| 5 | 0xAC | isCrewActive | Fixed(1) |
| 6 | 0xAD | customRoleSlotTypeId | Fixed(1) |
| 7 | 0xAE | arenaBonusType | Fixed(1) |
| 8 | 0xAF | isSpeedCapturing | Fixed(1) |
| 9 | 0xB0 | isBlockingCapture | Fixed(1) |
| 10 | 0xB1 | isMyVehicle | Fixed(1) |
| 11 | 0xB2 | enableExternalRespawn | Fixed(1) |
| 12 | 0xB3 | botDisplayStatus | Fixed(1) |
| 13 | 0xB4 | gunAnglesPacked | Fixed(2) |
| 14 | 0xB5 | health | Fixed(2) |
| 15 | 0xB6 | engineMode | Fixed(2) |
| 16 | 0xB7 | avatarID | Fixed(4) |
| 17 | 0xB8 | masterVehID | Fixed(4) |
| 18 | 0xB9 | arenaTypeID | Fixed(4) |
| 19 | 0xBA | debuff | Fixed(4) |
| 20 | 0xBB | quickShellChangerFactor | Fixed(4) |
| 21 | 0xBC | onRespawnReloadTimeFactor | Fixed(4) |
| 22 | 0xBD | wheelsState | Fixed(8) |
| 23 | 0xBE | stunInfo | Fixed(8) |
| 24 | 0xBF | arenaUniqueID | Fixed(8) |
| 25 | 0xC0 | remoteCamera | Fixed(22) |
| 26 | 0xC1 | steeringAngles | Variable8 |
| 27 | 0xC2 | wheelsScroll | Variable8 |
| 28 | 0xC3 | perkEffects | Variable8 |
| 29 | 0xC4 | perks | Variable8 |
| 30 | 0xC5 | perksRibbonNotify | Variable8 |
| 31 | 0xC6 | postmortemViewPointName | Variable8 |
| 32 | 0xC7 | publicInfo | Variable8 |
| 33 | 0xC8 | damageStickers | Variable8 |
| 34 | 0xC9 | publicStateModifiers | Variable8 |
| 35 | 0xCA | crewCompactDescrs | Variable8 |
| 36 | 0xCB | enhancements | Variable8 |
| 37 | 0xCC | setups | Variable8 |
| 38 | 0xCD | setupsIndexes | Variable8 |
| 39 | 0xCE | vehPerks | Variable8 |
| 40 | 0xCF | vehPostProgression | Variable8 |
| 41 | 0xD0 | disabledSwitches | Variable8 |
| 42 | 0xD1 | inspiringEffect | Variable8 |
| 43 | 0xD2 | healingEffect | Variable8 |
| 44 | 0xD3 | dotEffect | Variable8 |
| 45 | 0xD4 | inspired | Variable8 |
| 46 | 0xD5 | healing | Variable8 |
| 47 | 0xD6 | healOverTime | Variable8 |
| 48 | 0xD7 | dogTag | Variable8 |
| 49 | 0xD8 | ownVehiclePosition | Variable8 |

### client methods: 11

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x4E | onVehiclePickup | Fixed(0) |
| 1 | 0x4F | showShooting | Fixed(3) |
| 2 | 0x50 | onPushed | Fixed(8) |
| 3 | 0x51 | showAmmoBayEffect | Fixed(9) |
| 4 | 0x52 | onHealthChanged | Fixed(10) |
| 5 | 0x53 | onExtraHitted | Fixed(14) |
| 6 | 0x54 | showRammingEffect | Fixed(20) |
| 7 | 0x55 | showDamageFromExplosion | Fixed(23) |
| 8 | 0x56 | onStaticCollision | Fixed(36) |
| 9 | 0x57 | updateLaserSight | Variable8 |
| 10 | 0x58 | showDamageFromShot | Variable8 |

### cell methods: 11

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
| 0 | 0x10 | recoveryMechanic_startRecovering | Fixed(0) |
| 1 | 0x11 | recoveryMechanic_stopRecovering | Fixed(0) |
| 2 | 0x12 | sendStateToOwnClient | Fixed(0) |
| 3 | 0x13 | moveWith | Fixed(1) |
| 4 | 0x14 | switchSetup | Fixed(2) |
| 5 | 0x15 | changeSetting | Fixed(5) |
| 6 | 0x16 | stopTrackingWithGun | Fixed(8) |
| 7 | 0x17 | trackWorldPointWithGun | Fixed(12) |
| 8 | 0x18 | trackRelativePointWithGun | Fixed(12) |
| 9 | 0x19 | sendVisibilityDevelopmentInfo | Fixed(16) |
| 10 | 0x1A | setRemoteCamera | Fixed(22) |

### base methods: 0

| Exposed index | Element | Name | Framing |
| --- | --- | --- | --- |
