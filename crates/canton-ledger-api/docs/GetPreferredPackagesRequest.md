# GetPreferredPackagesRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**package_vetting_requirements** | Option<[**Vec<models::PackageVettingRequirement>**](PackageVettingRequirement.md)> | The package-name vetting requirements for which the preferred packages should be resolved.  Generally it is enough to provide the requirements for the intended command's root package-names. Additional package-name requirements can be provided when additional Daml transaction informees need to use package dependencies of the command's root packages.  Required | [optional]
**synchronizer_id** | **String** | The synchronizer whose vetting state to use for resolving this query. If not specified, the vetting state of all the synchronizers the participant is connected to will be used. Optional | 
**vetting_valid_at** | Option<**String**> | The timestamp at which the package vetting validity should be computed on the latest topology snapshot as seen by the participant. If not provided, the participant's current clock time is used. Optional | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


